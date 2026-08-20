//! `signature_lookup`: a selector to the signatures that hash to it.
//!
//! Turns an opaque `0x` blob into something `abi_decode` can be pointed at.
//! Uses the openchain.xyz signature database, which needs no key.
//!
//! A selector is four bytes, so collisions exist and are sometimes deliberate.
//! Every candidate is returned rather than one being picked.

use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    limits::parse_bytes,
    server::{EthData, error::ToolError},
};

const ENDPOINT: &str = "https://api.openchain.xyz/signature-database/v1/lookup";

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LookupParams {
    /// A 4-byte function selector such as `0xa9059cbb`, a 32-byte event topic,
    /// or full calldata whose first four bytes are the selector.
    pub selector: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LookupResult {
    /// What was actually looked up, after any calldata was trimmed to it.
    pub selector: String,
    pub kind: Kind,
    /// Every signature that hashes to this selector. More than one is normal.
    pub candidates: Vec<String>,
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Function,
    Event,
}

fn candidates_from(body: &Value, group: &str, selector: &str) -> Vec<String> {
    body.get("result")
        .and_then(|result| result.get(group))
        .and_then(|group| group.get(selector))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("name").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[tool_router(router = signature_router, vis = "pub(crate)")]
impl EthData {
    /// Look up the signatures that hash to a selector or an event topic.
    #[tool(name = "signature_lookup")]
    pub async fn signature_lookup(
        &self,
        Parameters(params): Parameters<LookupParams>,
    ) -> Result<Json<LookupResult>, ToolError> {
        let bytes = parse_bytes(&params.selector, "selector")?;

        // Calldata is accepted so a caller can paste a transaction's `input`
        // straight in without slicing it first.
        let (kind, selector) = match bytes.len() {
            32 => (Kind::Event, format!("0x{}", alloy::hex::encode(&bytes))),
            length if length >= 4 => (
                Kind::Function,
                format!("0x{}", alloy::hex::encode(&bytes[..4])),
            ),
            _ => {
                return Err(ToolError::invalid_input(
                    "A selector is 4 bytes and an event topic is 32.".to_string(),
                ));
            }
        };

        let group = match kind {
            Kind::Function => "function",
            Kind::Event => "event",
        };

        let response = crate::http_client()
            .get(ENDPOINT)
            .query(&[(group, selector.as_str()), ("filter", "true")])
            .header("Accept", "application/json")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(ToolError::upstream(format!(
                "openchain.xyz answered {}.",
                response.status()
            )));
        }

        let body: Value = response.json().await?;
        let candidates = candidates_from(&body, group, &selector);

        Ok(Json(LookupResult {
            note: match (candidates.len(), bytes.len()) {
                (0, _) => Some(
                    "No signature is registered for this selector. It may be a private \
                     function, or simply not submitted to the database."
                        .to_string(),
                ),
                (count, _) if count > 1 => Some(format!(
                    "{count} signatures hash to this selector. Decoding against the wrong \
                     one produces plausible nonsense, so check which fits the calldata length."
                )),
                _ => None,
            },
            selector,
            kind,
            candidates,
            source: "openchain.xyz",
        }))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn reads_candidates_out_of_the_response() {
        let body = json!({
            "ok": true,
            "result": {
                "function": {
                    "0xa9059cbb": [{ "name": "transfer(address,uint256)", "filtered": false }],
                },
                "event": {},
            },
        });
        assert_eq!(
            candidates_from(&body, "function", "0xa9059cbb"),
            vec!["transfer(address,uint256)"]
        );
    }

    #[test]
    fn an_unknown_selector_yields_no_candidates() {
        let body = json!({ "ok": true, "result": { "function": {}, "event": {} } });
        assert!(candidates_from(&body, "function", "0xdeadbeef").is_empty());
    }

    #[test]
    fn a_shape_we_do_not_recognise_yields_no_candidates_rather_than_panicking() {
        assert!(candidates_from(&json!({ "error": "nope" }), "function", "0x1").is_empty());
        assert!(candidates_from(&json!([]), "function", "0x1").is_empty());
    }
}
