//! Resolving a configured RPC URL into everything the tools need.

use std::{borrow::Cow, collections::BTreeMap, sync::Arc};

use alloy::{
    providers::{DynProvider, Provider, ProviderBuilder},
    rpc::{client::RpcClient, json_rpc::ErrorPayload},
    transports::{RpcError, http::Http},
};
#[cfg(feature = "traces")]
use serde_json::json;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::{
    config::{
        Config,
        derived::{self, ChainInfo},
    },
    server::error::ToolError,
};

/// Whether the configured endpoint serves a tracing namespace. Detected once,
/// with no fallback provider: a public endpoint that withholds `trace_` is
/// reported as unavailable rather than quietly answered by something else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
// Without the `traces` feature every network reports `None`, and the rest of
// the enum exists only so the reported shape does not change between builds.
#[cfg_attr(not(feature = "traces"), allow(dead_code))]
pub enum TraceSupport {
    /// The `trace_` namespace answered.
    Parity,
    /// The `debug_` namespace answered.
    Debug,
    /// Both answered. `trace_` is preferred because its output is smaller.
    Both,
    None,
}

#[cfg_attr(not(feature = "traces"), allow(dead_code))]
impl TraceSupport {
    pub fn has_parity(self) -> bool {
        matches!(self, Self::Parity | Self::Both)
    }

    pub fn has_debug(self) -> bool {
        matches!(self, Self::Debug | Self::Both)
    }

    pub fn any(self) -> bool {
        self.has_parity() || self.has_debug()
    }

    fn from_parts(parity: bool, debug: bool) -> Self {
        match (parity, debug) {
            (true, true) => Self::Both,
            (true, false) => Self::Parity,
            (false, true) => Self::Debug,
            (false, false) => Self::None,
        }
    }
}

/// A network that answered its chain ID probe.
#[derive(Clone)]
pub struct Ready {
    pub chain_id: u64,
    pub provider: DynProvider,
    /// `None` for a chain that is not in the built-in tables. Every RPC and
    /// fork tool still works; the groups that need a table do not.
    pub chain: Option<&'static ChainInfo>,
    pub traces: TraceSupport,
}

impl Ready {
    pub fn explorer(&self) -> Option<&'static str> {
        self.chain.and_then(|chain| chain.explorer)
    }

    pub fn label(&self) -> &'static str {
        self.chain
            .map(|chain| chain.label)
            .unwrap_or("unknown chain")
    }
}

enum State {
    Ready(Ready),
    Unavailable(String),
}

pub struct Network {
    pub name: String,
    pub rpc_url: String,
    state: RwLock<State>,
}

impl Network {
    /// Returns the resolved network, re-probing once if the endpoint was down
    /// when the server started. A public endpoint that recovers should not
    /// require restarting the editor the server runs inside.
    pub async fn ready(&self) -> Result<Ready, ToolError> {
        if let State::Ready(ready) = &*self.state.read().await {
            return Ok(ready.clone());
        }

        let mut state = self.state.write().await;
        if let State::Ready(ready) = &*state {
            return Ok(ready.clone());
        }

        match resolve(&self.rpc_url).await {
            Ok(ready) => {
                info!(network = %self.name, chain_id = ready.chain_id, "network recovered");
                *state = State::Ready(ready.clone());
                Ok(ready)
            }
            Err(reason) => {
                let message = format!("Network {:?} is unavailable: {reason}", self.name);
                *state = State::Unavailable(reason);
                Err(ToolError::upstream(message)
                    .with_remedy(format!("Endpoint: {}.", self.rpc_url)))
            }
        }
    }

    pub async fn availability(&self) -> Result<Ready, String> {
        match &*self.state.read().await {
            State::Ready(ready) => Ok(ready.clone()),
            State::Unavailable(reason) => Err(reason.clone()),
        }
    }
}

pub struct Networks {
    by_name: BTreeMap<String, Arc<Network>>,
}

impl Networks {
    /// Probes every configured endpoint concurrently. Startup only fails when
    /// none of them answered, because a server with one working network is
    /// still useful and a server that refuses to start is not.
    pub async fn resolve_all(config: &Config) -> Result<Self, String> {
        let probes = config.networks.iter().map(|(name, url)| async move {
            let state = match resolve(url).await {
                Ok(ready) => {
                    info!(
                        network = %name,
                        chain_id = ready.chain_id,
                        chain = ready.label(),
                        traces = ?ready.traces,
                        explorer = ready.explorer().is_some(),
                        "network ready",
                    );
                    State::Ready(ready)
                }
                Err(reason) => {
                    warn!(network = %name, %reason, "network unavailable at startup");
                    State::Unavailable(reason)
                }
            };
            (
                name.clone(),
                Arc::new(Network {
                    name: name.clone(),
                    rpc_url: url.clone(),
                    state: RwLock::new(state),
                }),
            )
        });

        let by_name: BTreeMap<String, Arc<Network>> = futures::future::join_all(probes)
            .await
            .into_iter()
            .collect();

        let any_ready =
            futures::future::join_all(by_name.values().map(|network| network.availability()))
                .await
                .into_iter()
                .any(|state| state.is_ok());

        if !any_ready {
            return Err(format!(
                "none of the {} configured networks answered: {}",
                by_name.len(),
                by_name.keys().cloned().collect::<Vec<_>>().join(", ")
            ));
        }

        Ok(Self { by_name })
    }

    pub fn names(&self) -> Vec<String> {
        self.by_name.keys().cloned().collect()
    }

    pub fn all(&self) -> impl Iterator<Item = &Arc<Network>> {
        self.by_name.values()
    }

    pub fn get(&self, name: &str) -> Result<&Arc<Network>, ToolError> {
        self.by_name
            .get(name)
            .ok_or_else(|| ToolError::unknown_network(name, &self.names()))
    }

    /// The common path: name to a resolved network, or an actionable error.
    pub async fn ready(&self, name: &str) -> Result<Ready, ToolError> {
        self.get(name)?.ready().await
    }
}

/// One provider construction path, so every outbound RPC carries the same
/// client limits.
pub fn provider_for(url: reqwest::Url) -> DynProvider {
    let local = matches!(
        url.host_str(),
        Some("127.0.0.1") | Some("localhost") | Some("::1")
    );
    let http = Http::with_client(crate::http_client(), url);
    ProviderBuilder::new()
        .connect_client(RpcClient::new(http, local))
        .erased()
}

async fn resolve(url: &str) -> Result<Ready, String> {
    let parsed = url
        .parse::<reqwest::Url>()
        .map_err(|error| format!("{url:?} is not a URL: {error}"))?;

    let provider = provider_for(parsed);

    let chain_id = provider
        .get_chain_id()
        .await
        .map_err(|error| format!("eth_chainId failed: {error}"))?;

    let traces = probe_traces(&provider).await;

    Ok(Ready {
        chain_id,
        provider,
        chain: derived::lookup(chain_id),
        traces,
    })
}

#[cfg(feature = "traces")]
async fn probe_traces(provider: &DynProvider) -> TraceSupport {
    let parity = namespace_answers(provider, "trace_block", json!(["0x0"])).await;
    let debug = namespace_answers(
        provider,
        "debug_traceTransaction",
        json!([format!("0x{}", "0".repeat(64))]),
    )
    .await;
    TraceSupport::from_parts(parity, debug)
}

#[cfg(not(feature = "traces"))]
async fn probe_traces(_provider: &DynProvider) -> TraceSupport {
    TraceSupport::None
}

/// A method that exists but rejects these arguments still proves the namespace
/// is served. Only an explicit "no such method" counts as absent.
#[cfg_attr(not(feature = "traces"), allow(dead_code))]
async fn namespace_answers(
    provider: &DynProvider,
    method: &'static str,
    params: serde_json::Value,
) -> bool {
    match provider
        .raw_request::<serde_json::Value, serde_json::Value>(Cow::Borrowed(method), params)
        .await
    {
        Ok(_) => true,
        Err(RpcError::ErrorResp(payload)) => !is_method_missing(&payload),
        Err(_) => false,
    }
}

fn is_method_missing(payload: &ErrorPayload) -> bool {
    const METHOD_NOT_FOUND: i64 = -32601;
    if payload.code == METHOD_NOT_FOUND {
        return true;
    }
    let message = payload.message.to_ascii_lowercase();
    [
        "method not found",
        "not available",
        "does not exist",
        "unsupported method",
    ]
    .iter()
    .any(|marker| message.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(code: i64, message: &str) -> ErrorPayload {
        ErrorPayload {
            code,
            message: message.to_string().into(),
            data: None,
        }
    }

    #[test]
    fn method_not_found_code_means_absent() {
        assert!(is_method_missing(&payload(-32601, "whatever")));
    }

    #[test]
    fn a_wrong_argument_does_not_mean_absent() {
        assert!(!is_method_missing(&payload(-32602, "invalid argument")));
        assert!(!is_method_missing(&payload(
            -32000,
            "transaction not found"
        )));
    }

    #[test]
    fn a_worded_rejection_means_absent() {
        assert!(is_method_missing(&payload(
            -32000,
            "The method does not exist"
        )));
        assert!(is_method_missing(&payload(
            -32000,
            "trace_block is not available"
        )));
    }

    #[test]
    fn trace_support_combines() {
        assert_eq!(TraceSupport::from_parts(true, true), TraceSupport::Both);
        assert_eq!(TraceSupport::from_parts(false, false), TraceSupport::None);
        assert!(TraceSupport::Both.has_parity());
        assert!(TraceSupport::Both.has_debug());
        assert!(!TraceSupport::Debug.has_parity());
    }
}
