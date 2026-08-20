//! Local Anvil forks: start one, find it, watch it, stop it.
//!
//! The Python server wrapped 18 tools around a fork, 14 of which were one-line
//! proxies onto an Anvil RPC method. Those are gone. What a caller gets instead
//! is the fork's RPC URL and its funded accounts, which `cast`, `forge`, and any
//! script in the repository already know how to use. What stays here is the
//! part a shell cannot do for itself: making sure the child process dies when
//! the server does.

use alloy::providers::Provider;
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{
    limits::MAX_FORK_LOG_REQUEST,
    server::{EthData, error::ToolError},
};

pub mod anvil;
pub mod registry;

pub use registry::{Fork, Registry};

/// Repeated in every fork result, because the alternative is a caller that has
/// a URL and no idea it is allowed to use it.
const HANDOVER: &str = "Drive this fork with your own tools against rpc_url, for example \
`cast rpc anvil_setBalance <addr> <wei> --rpc-url <rpc_url>`, \
`cast send <to> <sig> --unlocked --from <addr> --rpc-url <rpc_url>`, or \
`forge script <path> --rpc-url <rpc_url> --broadcast`. \
There are no eth-data tools for those; this server does not write state.";

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateParams {
    /// A name from `network_list`. Its RPC URL becomes the fork source.
    pub network: String,
    /// Fork at this height instead of the chain head. Pin it when you want a
    /// run to be reproducible.
    pub block_number: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForkParams {
    /// An id from `fork_create` or `fork_list`.
    pub fork_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LogsParams {
    /// An id from `fork_create` or `fork_list`.
    pub fork_id: String,
    /// How many lines from the end to return. Defaults to 50, capped at 200.
    pub lines: Option<usize>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ForkCreated {
    pub fork_id: String,
    /// The endpoint to point `cast`, `forge`, or a script at.
    pub rpc_url: String,
    pub port: u16,
    pub network: String,
    pub chain_id: u64,
    /// The height the fork was taken at, when one was pinned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fork_block_number: Option<u64>,
    pub accounts: Vec<Account>,
    pub how_to_use: &'static str,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Account {
    pub address: String,
    /// A well-known Anvil development key. It funds nothing outside this local
    /// fork, and is safe to put on a command line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ForkSummary {
    pub fork_id: String,
    pub rpc_url: String,
    pub port: u16,
    pub network: String,
    pub chain_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fork_block_number: Option<u64>,
    pub alive: bool,
    pub age_seconds: u64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ForkList {
    pub forks: Vec<ForkSummary>,
    pub how_to_use: &'static str,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ForkStatus {
    #[serde(flatten)]
    pub summary: ForkSummary,
    /// The fork's current head, which advances as you mine or send.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_block: Option<u64>,
    pub how_to_use: &'static str,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ForkAccounts {
    pub fork_id: String,
    pub rpc_url: String,
    pub accounts: Vec<Account>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ForkLogs {
    pub fork_id: String,
    pub lines: Vec<String>,
    /// True when older lines have already been dropped from the buffer.
    pub truncated: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ForkClosed {
    pub fork_id: String,
    pub stopped: bool,
}

fn accounts_of(fork: &Fork) -> Vec<Account> {
    fork.addresses
        .iter()
        .enumerate()
        .map(|(index, address)| Account {
            address: address.clone(),
            private_key: fork.private_keys.get(index).cloned(),
        })
        .collect()
}

async fn summary_of(fork: &Fork) -> ForkSummary {
    ForkSummary {
        fork_id: fork.id.clone(),
        rpc_url: fork.rpc_url.clone(),
        port: fork.port,
        network: fork.network.clone(),
        chain_id: fork.chain_id,
        fork_block_number: fork.fork_block,
        alive: fork.is_alive().await,
        age_seconds: fork.age_seconds(),
    }
}

#[tool_router(router = fork_router, vis = "pub(crate)")]
impl EthData {
    /// Start an isolated Anvil fork of a configured network and return the
    /// loopback RPC URL to drive it at.
    ///
    /// Everything you do to the fork afterwards goes through your own tools
    /// against that URL: `cast`, `forge`, or a script in the repository. There
    /// are deliberately no tools here for setting balances, impersonating, or
    /// sending, because `cast` already does all three with better arguments.
    #[tool(name = "fork_create")]
    pub async fn fork_create(
        &self,
        Parameters(params): Parameters<CreateParams>,
    ) -> Result<Json<ForkCreated>, ToolError> {
        // Resolving first means a bad network name fails before a process is
        // started, rather than after.
        let network = self.state.networks.get(&params.network)?;
        network.ready().await?;

        let id = self.state.forks.next_id();
        let fork = Fork::start(
            id,
            params.network.clone(),
            &network.rpc_url,
            params.block_number,
        )
        .await?;
        let fork = self.state.forks.insert(fork).await?;

        info!(
            fork = %fork.id,
            port = fork.port,
            network = %fork.network,
            "fork started",
        );

        Ok(Json(ForkCreated {
            fork_id: fork.id.clone(),
            rpc_url: fork.rpc_url.clone(),
            port: fork.port,
            network: fork.network.clone(),
            chain_id: fork.chain_id,
            fork_block_number: fork.fork_block,
            accounts: accounts_of(&fork),
            how_to_use: HANDOVER,
        }))
    }

    /// List every fork this server is running, with the URL for each.
    #[tool(name = "fork_list")]
    pub async fn fork_list(&self) -> Result<Json<ForkList>, ToolError> {
        let forks = self.state.forks.list().await;
        let mut summaries = Vec::with_capacity(forks.len());
        for fork in &forks {
            summaries.push(summary_of(fork).await);
        }
        Ok(Json(ForkList {
            forks: summaries,
            how_to_use: HANDOVER,
        }))
    }

    /// Check whether a fork is still alive and where its head is.
    #[tool(name = "fork_status")]
    pub async fn fork_status(
        &self,
        Parameters(params): Parameters<ForkParams>,
    ) -> Result<Json<ForkStatus>, ToolError> {
        let fork = self.state.forks.get(&params.fork_id).await?;
        let summary = summary_of(&fork).await;
        let head_block = if summary.alive {
            fork.provider.get_block_number().await.ok()
        } else {
            None
        };

        Ok(Json(ForkStatus {
            summary,
            head_block,
            how_to_use: HANDOVER,
        }))
    }

    /// List the fork's funded development accounts and their keys.
    #[tool(name = "fork_accounts")]
    pub async fn fork_accounts(
        &self,
        Parameters(params): Parameters<ForkParams>,
    ) -> Result<Json<ForkAccounts>, ToolError> {
        let fork = self.state.forks.get(&params.fork_id).await?;
        Ok(Json(ForkAccounts {
            fork_id: fork.id.clone(),
            rpc_url: fork.rpc_url.clone(),
            accounts: accounts_of(&fork),
        }))
    }

    /// Read the tail of the fork's Anvil output.
    ///
    /// Anvil logs every RPC call it serves, so this is where you see what your
    /// own `cast` and `forge` commands did against the fork, and why one of
    /// them failed.
    #[tool(name = "fork_logs")]
    pub async fn fork_logs(
        &self,
        Parameters(params): Parameters<LogsParams>,
    ) -> Result<Json<ForkLogs>, ToolError> {
        let fork = self.state.forks.get(&params.fork_id).await?;
        let requested = params.lines.unwrap_or(50).min(MAX_FORK_LOG_REQUEST);
        let lines = fork.logs.tail(requested);
        let available = fork.logs.tail(usize::MAX).len();

        Ok(Json(ForkLogs {
            fork_id: fork.id.clone(),
            truncated: available > lines.len(),
            lines,
        }))
    }

    /// Stop a fork and release its port.
    #[tool(name = "fork_close")]
    pub async fn fork_close(
        &self,
        Parameters(params): Parameters<ForkParams>,
    ) -> Result<Json<ForkClosed>, ToolError> {
        let fork = self.state.forks.close(&params.fork_id).await?;
        Ok(Json(ForkClosed {
            fork_id: fork.id.clone(),
            stopped: true,
        }))
    }
}
