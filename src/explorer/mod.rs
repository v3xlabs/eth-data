//! Indexed discovery through a public Blockscout instance.
//!
//! The instance is derived from the network's chain ID, not configured, so
//! there is no explorer URL for a user to get wrong. A chain with no known
//! instance reports `unsupported_on_network` rather than silently returning
//! nothing, and never falls back to an unrelated service.

use futures::StreamExt;
use rmcp::handler::server::router::tool::ToolRouter;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::{
    config::network::Ready,
    limits::{MAX_RESPONSE_BYTES, check_cursor, check_page_size},
    server::{EthData, error::ToolError},
};

pub mod address;
pub mod balances;
pub mod contract;
pub mod nft;
pub mod token;

impl EthData {
    pub(crate) fn explorer_router() -> ToolRouter<Self> {
        Self::address_router()
            + Self::balances_router()
            + Self::contract_router()
            + Self::token_router()
            + Self::nft_router()
    }
}

/// One bounded page, plus the cursor for the next one.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Page {
    pub items: Vec<Value>,
    pub count: usize,
    /// Pass this back as `cursor` to get the next page. Absent means this was
    /// the last one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// The instance these items came from.
    pub source: String,
}

pub(crate) fn require_explorer(ready: &Ready, network: &str) -> Result<&'static str, ToolError> {
    ready.explorer().ok_or_else(|| {
        ToolError::unsupported(format!(
            "No Blockscout instance is known for chain {} ({network:?}).",
            ready.chain_id
        ))
        .with_remedy(
            "Explorer tools need one. Chain reads, ENS, prices, and forks still work. \
             Adding an instance is a change to eth-data, not to your config."
                .to_string(),
        )
    })
}

/// Blockscout paginates by echoing a `next_page_params` object back as query
/// parameters. Carrying it as an opaque JSON string keeps that detail out of
/// every tool signature.
fn cursor_params(cursor: Option<&str>) -> Result<Map<String, Value>, ToolError> {
    let Some(cursor) = check_cursor(cursor)? else {
        return Ok(Map::new());
    };
    match serde_json::from_str::<Value>(&cursor) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(ToolError::invalid_input(
            "cursor must be a value returned as next_cursor by a previous call.".to_string(),
        )),
    }
}

fn next_cursor(body: &Value) -> Option<String> {
    match body.get("next_page_params") {
        Some(Value::Object(map)) if !map.is_empty() => serde_json::to_string(map).ok(),
        _ => None,
    }
}

fn as_items(body: &Value) -> Vec<Value> {
    match body.get("items") {
        Some(Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    }
}

pub(crate) async fn get(
    base: &str,
    path: &str,
    query: &[(String, String)],
) -> Result<Value, ToolError> {
    let url = format!("{base}{path}");
    let response = crate::http_client()
        .get(&url)
        .query(query)
        .header("Accept", "application/json")
        .send()
        .await?;

    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(ToolError::upstream(format!(
            "The explorer has no record at {path}."
        )));
    }
    if !status.is_success() {
        return Err(ToolError::upstream(format!(
            "The explorer answered {status} for {path}."
        )));
    }

    // Read with a cap rather than trusting Content-Length, which a chunked
    // response does not carry.
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(ToolError::upstream(format!(
                "The explorer response for {path} exceeded {MAX_RESPONSE_BYTES} bytes."
            ))
            .with_remedy("Request a smaller page_size.".to_string()));
        }
        body.extend_from_slice(&chunk);
    }

    serde_json::from_slice(&body).map_err(|error| {
        ToolError::upstream(format!("The explorer returned invalid JSON: {error}"))
    })
}

/// A paged collection endpoint. Every list tool in this group goes through here.
pub(crate) async fn page(
    ready: &Ready,
    network: &str,
    path: &str,
    page_size: Option<u32>,
    cursor: Option<&str>,
    extra: &[(&str, &str)],
) -> Result<Page, ToolError> {
    let base = require_explorer(ready, network)?;
    let size = check_page_size(page_size)?;

    let mut query: Vec<(String, String)> = vec![("items_count".to_string(), size.to_string())];
    for (key, value) in extra {
        query.push(((*key).to_string(), (*value).to_string()));
    }
    for (key, value) in cursor_params(cursor)? {
        let rendered = match value {
            Value::String(text) => text,
            other => other.to_string(),
        };
        query.push((key, rendered));
    }

    let body = get(base, path, &query).await?;
    let mut items = as_items(&body);
    // Blockscout honours items_count loosely. Enforcing it here is what makes
    // the page size a promise rather than a hint.
    items.truncate(size as usize);

    Ok(Page {
        count: items.len(),
        next_cursor: next_cursor(&body),
        items,
        source: base.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_cursor_round_trips_through_json() {
        let body = json!({
            "items": [],
            "next_page_params": { "block_number": 100, "index": 3 },
        });
        let cursor = next_cursor(&body).unwrap();
        let params = cursor_params(Some(&cursor)).unwrap();
        assert_eq!(params.get("block_number"), Some(&json!(100)));
        assert_eq!(params.get("index"), Some(&json!(3)));
    }

    #[test]
    fn the_last_page_has_no_cursor() {
        assert!(next_cursor(&json!({ "items": [], "next_page_params": null })).is_none());
        assert!(next_cursor(&json!({ "items": [], "next_page_params": {} })).is_none());
        assert!(next_cursor(&json!({ "items": [] })).is_none());
    }

    #[test]
    fn a_hand_written_cursor_is_rejected() {
        assert!(cursor_params(Some("not json")).is_err());
        assert!(cursor_params(Some("[1,2,3]")).is_err());
    }

    #[test]
    fn missing_items_reads_as_empty_rather_than_failing() {
        assert!(as_items(&json!({ "message": "not found" })).is_empty());
    }
}
