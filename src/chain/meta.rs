//! `network_list` and `network_status`: what exists, and what it can do.

use std::borrow::Cow;

use alloy::providers::Provider;
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    chain::rpc_error,
    config::network::TraceSupport,
    server::{
        EthData,
        error::ToolError,
        output::{Amount, BlockContext},
    },
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NetworkStatusParams {
    /// A name from `network_list`.
    pub network: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct NetworkSummary {
    pub network: String,
    pub rpc_url: String,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain: Option<String>,
    /// Which tool groups this chain unlocked, and why the others are dark.
    pub capabilities: Capabilities,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Capabilities {
    /// Explorer tools work. Derived from the chain ID, not configured.
    pub explorer: bool,
    /// `trace_tx` and `trace_call` work.
    pub traces: bool,
    pub trace_namespace: TraceSupport,
    /// `price_get` can reach `fiat:usd` on this chain.
    pub prices: bool,
    /// The server was built with fork support and `anvil` should be on PATH.
    pub forks: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct NetworkList {
    pub networks: Vec<NetworkSummary>,
    /// Tool groups compiled into this binary.
    pub features: Vec<&'static str>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct NetworkStatus {
    pub network: String,
    pub chain_id: u64,
    pub chain: String,
    pub client_version: String,
    pub head: BlockContext,
    pub gas_price: Amount,
    pub capabilities: Capabilities,
}

pub const COMPILED_FEATURES: &[&str] = &[
    "chain",
    #[cfg(feature = "traces")]
    "traces",
    #[cfg(feature = "ens")]
    "ens",
    #[cfg(feature = "explorer")]
    "explorer",
    #[cfg(feature = "signatures")]
    "signatures",
    #[cfg(feature = "icons")]
    "icons",
    #[cfg(feature = "avatars")]
    "avatars",
    #[cfg(feature = "prices")]
    "prices",
    #[cfg(feature = "fork")]
    "fork",
];

fn capabilities(ready: &crate::config::network::Ready) -> Capabilities {
    Capabilities {
        explorer: cfg!(feature = "explorer") && ready.explorer().is_some(),
        traces: cfg!(feature = "traces") && ready.traces != TraceSupport::None,
        trace_namespace: ready.traces,
        prices: cfg!(feature = "prices")
            && ready
                .chain
                .map(|chain| chain.usd_anchor().is_some())
                .unwrap_or(false),
        forks: cfg!(feature = "fork"),
    }
}

#[tool_router(router = meta_router, vis = "pub(crate)")]
impl EthData {
    /// List configured networks, the chain each endpoint resolved to, and what
    /// that chain unlocked. Start here: it is the one place that explains why a
    /// tool would answer `unsupported_on_network`.
    #[tool(name = "network_list")]
    pub async fn network_list(&self) -> Result<Json<NetworkList>, ToolError> {
        let mut networks = Vec::new();
        for network in self.state.networks.all() {
            let summary = match network.availability().await {
                Ok(ready) => NetworkSummary {
                    network: network.name.clone(),
                    rpc_url: network.rpc_url.clone(),
                    available: true,
                    unavailable_reason: None,
                    chain_id: Some(ready.chain_id),
                    chain: Some(ready.label().to_string()),
                    capabilities: capabilities(&ready),
                },
                Err(reason) => NetworkSummary {
                    network: network.name.clone(),
                    rpc_url: network.rpc_url.clone(),
                    available: false,
                    unavailable_reason: Some(reason),
                    chain_id: None,
                    chain: None,
                    capabilities: Capabilities {
                        explorer: false,
                        traces: false,
                        trace_namespace: TraceSupport::None,
                        prices: false,
                        forks: cfg!(feature = "fork"),
                    },
                },
            };
            networks.push(summary);
        }

        Ok(Json(NetworkList {
            networks,
            features: COMPILED_FEATURES.to_vec(),
        }))
    }

    /// Chain identity, client version, head block, and gas price for one network.
    #[tool(name = "network_status")]
    pub async fn network_status(
        &self,
        Parameters(params): Parameters<NetworkStatusParams>,
    ) -> Result<Json<NetworkStatus>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;

        let (head, gas_price, client_version) = futures::join!(
            ready.provider.get_block_number(),
            ready.provider.get_gas_price(),
            ready
                .provider
                .raw_request::<[(); 0], String>(Cow::Borrowed("web3_clientVersion"), []),
        );

        let head = head.map_err(|error| rpc_error("eth_blockNumber", error))?;
        let gas_price = gas_price.map_err(|error| rpc_error("eth_gasPrice", error))?;

        Ok(Json(NetworkStatus {
            network: params.network,
            chain_id: ready.chain_id,
            chain: ready.label().to_string(),
            // Not every endpoint serves web3_clientVersion, and its absence is
            // not a reason to fail a status call.
            client_version: client_version.unwrap_or_else(|_| "unreported".to_string()),
            head: BlockContext::number(head),
            gas_price: Amount::new(
                alloy::primitives::U256::from(gas_price),
                18,
                Some(crate::chain::native_symbol(&ready).to_string()),
            ),
            capabilities: capabilities(&ready),
        }))
    }
}
