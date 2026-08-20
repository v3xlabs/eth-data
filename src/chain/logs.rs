//! `logs_get`, over a range bounded before the request leaves the process.

use alloy::{primitives::B256, providers::Provider, rpc::types::Filter};
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    chain::rpc_error,
    limits::{MAX_LOG_BLOCK_RANGE, MAX_LOG_TOPICS, check_log_range, parse_address, parse_hash},
    server::{EthData, error::ToolError},
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LogsParams {
    /// A name from `network_list`.
    pub network: String,
    /// First block in the range, inclusive.
    pub from_block: u64,
    /// Last block in the range, inclusive. At most 10,000 blocks after
    /// `from_block`; a wider range is rejected before anything is sent.
    pub to_block: u64,
    /// Restrict to logs emitted by this contract.
    pub address: Option<String>,
    /// Up to four topic filters, positionally. Use `null` for a wildcard in a
    /// position you do not want to constrain. Topic 0 is the event signature
    /// hash; `signature_lookup` can tell you what one is.
    pub topics: Option<Vec<Option<String>>>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LogsResult {
    pub network: String,
    pub from_block: u64,
    pub to_block: u64,
    pub count: usize,
    pub logs: Vec<Value>,
}

#[tool_router(router = logs_router, vis = "pub(crate)")]
impl EthData {
    /// Read logs over a bounded block range.
    #[tool(name = "logs_get")]
    pub async fn logs_get(
        &self,
        Parameters(params): Parameters<LogsParams>,
    ) -> Result<Json<LogsResult>, ToolError> {
        check_log_range(params.from_block, params.to_block)?;
        let ready = self.state.networks.ready(&params.network).await?;

        let mut filter = Filter::new()
            .from_block(params.from_block)
            .to_block(params.to_block);

        if let Some(address) = &params.address {
            filter = filter.address(parse_address(address)?);
        }

        if let Some(topics) = &params.topics {
            if topics.len() > MAX_LOG_TOPICS {
                return Err(ToolError::invalid_input(format!(
                    "At most {MAX_LOG_TOPICS} topics are supported, received {}.",
                    topics.len()
                )));
            }
            for (position, topic) in topics.iter().enumerate() {
                let Some(topic) = topic else { continue };
                let hash: B256 = parse_hash(topic, &format!("topic {position}"))?;
                filter = match position {
                    0 => filter.event_signature(hash),
                    1 => filter.topic1(hash),
                    2 => filter.topic2(hash),
                    _ => filter.topic3(hash),
                };
            }
        }

        let logs =
            ready
                .provider
                .get_logs(&filter)
                .await
                .map_err(|error| match error.to_string() {
                    // Public endpoints commonly cap ranges below ours. Say which
                    // limit was hit rather than passing the provider string on.
                    text if text.contains("query returned more than")
                        || text.contains("range")
                        || text.contains("limit") =>
                    {
                        ToolError::upstream(format!(
                            "The endpoint refused this range: {text}. \
                         eth-data allows up to {MAX_LOG_BLOCK_RANGE} blocks; \
                         this provider allows fewer. Query a narrower range."
                        ))
                    }
                    _ => rpc_error("eth_getLogs", error),
                })?;

        let values = logs
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| ToolError::upstream(format!("Malformed log: {error}")))?;

        Ok(Json(LogsResult {
            network: params.network,
            from_block: params.from_block,
            to_block: params.to_block,
            count: values.len(),
            logs: values,
        }))
    }
}
