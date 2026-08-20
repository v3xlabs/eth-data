//! `trace_tx` and `trace_call`, only where the endpoint serves them.
//!
//! Trace output shape differs between the `trace_` and `debug_` namespaces and
//! between node implementations, so the node's own JSON is returned verbatim
//! with the namespace named. Reshaping it would invent a schema that no node
//! actually promises.

use std::borrow::Cow;

use alloy::providers::Provider;
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    chain::{block_id, rpc_error},
    config::network::Ready,
    limits::{MAX_RESPONSE_BYTES, parse_address, parse_bytes, parse_hash, parse_u256},
    server::{EthData, error::ToolError},
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TraceTxParams {
    /// A name from `network_list`. Check `capabilities.traces` first.
    pub network: String,
    /// 32-byte transaction hash.
    pub transaction_hash: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TraceCallParams {
    /// A name from `network_list`. Check `capabilities.traces` first.
    pub network: String,
    /// The contract to call.
    pub to: String,
    /// ABI-encoded calldata.
    pub data: Option<String>,
    /// The caller to simulate as.
    pub from: Option<String>,
    /// Native value to attach, decimal or 0x-prefixed wei.
    pub value: Option<String>,
    /// Block number, tag, or hash. Defaults to `latest`.
    pub block: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TraceResult {
    pub network: String,
    /// Which namespace answered. The shape of `trace` follows from this.
    pub namespace: &'static str,
    pub method: &'static str,
    /// True when the trace was too large to return and was dropped. Narrow the
    /// call rather than expecting a partial tree.
    pub truncated: bool,
    pub trace: Value,
}

fn require_traces(ready: &Ready, network: &str) -> Result<(), ToolError> {
    if !ready.traces.any() {
        return Err(ToolError::unsupported(format!(
            "The endpoint configured for {network:?} serves neither the trace_ nor the debug_ namespace."
        ))
        .with_remedy(
            "Public endpoints commonly withhold both. Point this network at a node that serves them."
                .to_string(),
        ));
    }
    Ok(())
}

/// A trace of a heavy transaction can be tens of megabytes. Returning it would
/// blow the caller's context open for no benefit, so it is refused by size.
fn bound(
    network: String,
    namespace: &'static str,
    method: &'static str,
    trace: Value,
) -> TraceResult {
    let size = serde_json::to_string(&trace)
        .map(|text| text.len())
        .unwrap_or(0);
    if size > MAX_RESPONSE_BYTES {
        return TraceResult {
            network,
            namespace,
            method,
            truncated: true,
            trace: json!({
                "error": format!(
                    "The trace was {size} bytes, over the {MAX_RESPONSE_BYTES} byte limit."
                ),
            }),
        };
    }
    TraceResult {
        network,
        namespace,
        method,
        truncated: false,
        trace,
    }
}

fn call_object(
    to: &str,
    data: Option<&str>,
    from: Option<&str>,
    value: Option<&str>,
) -> Result<Value, ToolError> {
    let mut object = serde_json::Map::new();
    object.insert("to".into(), json!(parse_address(to)?.to_string()));
    if let Some(data) = data {
        object.insert("data".into(), json!(parse_bytes(data, "data")?.to_string()));
    }
    if let Some(from) = from {
        object.insert("from".into(), json!(parse_address(from)?.to_string()));
    }
    if let Some(value) = value {
        let parsed = parse_u256(value, "value")?;
        object.insert("value".into(), json!(format!("0x{parsed:x}")));
    }
    Ok(Value::Object(object))
}

#[tool_router(router = trace_router, vis = "pub(crate)")]
impl EthData {
    /// Trace one executed transaction. Requires an endpoint that serves the
    /// `trace_` or `debug_` namespace; `network_list` reports which do.
    #[tool(name = "trace_tx")]
    pub async fn trace_tx(
        &self,
        Parameters(params): Parameters<TraceTxParams>,
    ) -> Result<Json<TraceResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        require_traces(&ready, &params.network)?;
        let hash = parse_hash(&params.transaction_hash, "transaction_hash")?;

        let (namespace, method, request) = if ready.traces.has_parity() {
            ("trace", "trace_transaction", json!([hash.to_string()]))
        } else {
            (
                "debug",
                "debug_traceTransaction",
                json!([hash.to_string(), { "tracer": "callTracer" }]),
            )
        };

        let trace = ready
            .provider
            .raw_request::<Value, Value>(Cow::Borrowed(method), request)
            .await
            .map_err(|error| rpc_error(method, error))?;

        Ok(Json(bound(params.network, namespace, method, trace)))
    }

    /// Trace a read-only call without sending it.
    #[tool(name = "trace_call")]
    pub async fn trace_call(
        &self,
        Parameters(params): Parameters<TraceCallParams>,
    ) -> Result<Json<TraceResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        require_traces(&ready, &params.network)?;

        let call = call_object(
            &params.to,
            params.data.as_deref(),
            params.from.as_deref(),
            params.value.as_deref(),
        )?;
        // Validate the block argument even though it is passed through as the
        // caller wrote it.
        block_id(params.block.as_deref())?;
        let block = params.block.clone().unwrap_or_else(|| "latest".to_string());

        let (namespace, method, request) = if ready.traces.has_parity() {
            ("trace", "trace_call", json!([call, ["trace"], block]))
        } else {
            (
                "debug",
                "debug_traceCall",
                json!([call, block, { "tracer": "callTracer" }]),
            )
        };

        let trace = ready
            .provider
            .raw_request::<Value, Value>(Cow::Borrowed(method), request)
            .await
            .map_err(|error| rpc_error(method, error))?;

        Ok(Json(bound(params.network, namespace, method, trace)))
    }
}
