//! Fork ownership. The one thing a shell cannot do for itself: making sure a
//! child process dies when the server does.

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use alloy::providers::{DynProvider, Provider};
use tokio::{process::Child, sync::Mutex};
use tracing::{info, warn};

use crate::{
    fork::anvil::LogBuffer,
    limits::{FORK_STOP_TIMEOUT, MAX_LIVE_FORKS},
    server::error::ToolError,
};

pub struct Fork {
    pub id: String,
    pub network: String,
    pub fork_block: Option<u64>,
    pub port: u16,
    pub rpc_url: String,
    pub chain_id: u64,
    pub provider: DynProvider,
    pub addresses: Vec<String>,
    pub private_keys: Vec<String>,
    pub logs: LogBuffer,
    started: Instant,
    child: Mutex<Option<Child>>,
}

impl Fork {
    pub fn age_seconds(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// True while the child is still running. A fork whose Anvil died is not a
    /// fork, and saying so beats a connection refused from the caller's shell.
    pub async fn is_alive(&self) -> bool {
        let mut child = self.child.lock().await;
        match child.as_mut() {
            None => false,
            Some(process) => matches!(process.try_wait(), Ok(None)),
        }
    }

    async fn stop(&self) {
        let Some(mut process) = self.child.lock().await.take() else {
            return;
        };
        if matches!(process.try_wait(), Ok(Some(_))) {
            return;
        }
        if let Err(error) = process.start_kill() {
            warn!(fork = %self.id, %error, "could not signal anvil");
            return;
        }
        match tokio::time::timeout(FORK_STOP_TIMEOUT, process.wait()).await {
            Ok(_) => info!(fork = %self.id, port = self.port, "fork stopped"),
            Err(_) => {
                warn!(fork = %self.id, "anvil did not exit in time, killing");
                let _ = process.kill().await;
            }
        }
    }
}

#[derive(Default)]
pub struct Registry {
    forks: Mutex<BTreeMap<String, Arc<Fork>>>,
    counter: AtomicU64,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn next_id(&self) -> String {
        format!("fork-{}", self.counter.fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// Refuses past the limit rather than letting a loop fill the machine with
    /// forked mainnet state.
    pub async fn insert(&self, fork: Fork) -> Result<Arc<Fork>, ToolError> {
        let mut forks = self.forks.lock().await;
        if forks.len() >= MAX_LIVE_FORKS {
            return Err(
                ToolError::fork(format!("{MAX_LIVE_FORKS} forks are already running."))
                    .with_remedy(
                        "Close one with fork_close, or list them with fork_list.".to_string(),
                    ),
            );
        }
        let fork = Arc::new(fork);
        forks.insert(fork.id.clone(), Arc::clone(&fork));
        Ok(fork)
    }

    pub async fn get(&self, id: &str) -> Result<Arc<Fork>, ToolError> {
        let forks = self.forks.lock().await;
        forks.get(id).cloned().ok_or_else(|| {
            let known: Vec<&str> = forks.keys().map(String::as_str).collect();
            ToolError::fork(format!("No fork named {id:?} is running.")).with_remedy(
                if known.is_empty() {
                    "No forks are running. Start one with fork_create.".to_string()
                } else {
                    format!("Running forks: {}.", known.join(", "))
                },
            )
        })
    }

    pub async fn list(&self) -> Vec<Arc<Fork>> {
        self.forks.lock().await.values().cloned().collect()
    }

    pub async fn close(&self, id: &str) -> Result<Arc<Fork>, ToolError> {
        let fork = self.get(id).await?;
        fork.stop().await;
        self.forks.lock().await.remove(id);
        Ok(fork)
    }

    /// Called once as the server stops. A fork that outlives its server is an
    /// orphan holding a port and a few hundred megabytes of forked state.
    pub async fn shutdown(&self) {
        let forks: Vec<Arc<Fork>> = self.forks.lock().await.values().cloned().collect();
        if forks.is_empty() {
            return;
        }
        info!(count = forks.len(), "stopping forks");
        futures::future::join_all(forks.iter().map(|fork| fork.stop())).await;
        self.forks.lock().await.clear();
    }
}

impl Fork {
    /// Starts Anvil, waits for it to listen, and reads back everything the
    /// caller needs to drive it from a shell.
    pub async fn start(
        id: String,
        network: String,
        fork_url: &str,
        fork_block: Option<u64>,
    ) -> Result<Self, ToolError> {
        let spawned = crate::fork::anvil::spawn(fork_url, fork_block).await?;
        let rpc_url = format!("http://127.0.0.1:{}", spawned.port);

        let url = rpc_url
            .parse::<reqwest::Url>()
            .expect("a loopback URL built from a u16 port always parses");
        let provider = crate::config::network::provider_for(url);

        // Anvil listening does not mean Anvil is answering. This is the health
        // check, and it also gets the two facts every later call needs.
        let (chain_id, accounts) = futures::join!(provider.get_chain_id(), provider.get_accounts());

        let chain_id = chain_id.map_err(|error| {
            ToolError::fork(format!(
                "The fork started but did not answer eth_chainId: {error}"
            ))
        })?;

        Ok(Self {
            id,
            network,
            fork_block,
            port: spawned.port,
            rpc_url,
            chain_id,
            provider,
            addresses: accounts
                .unwrap_or_default()
                .into_iter()
                .map(|address| address.to_string())
                .collect(),
            private_keys: spawned.private_keys,
            logs: spawned.logs,
            started: Instant::now(),
            child: Mutex::new(Some(spawned.child)),
        })
    }
}
