//! `contract_abi` and `contract_source`.
//!
//! An unverified contract reports as unverified. It does not report as an
//! error, and it does not report an empty ABI as though that were the answer.

use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    explorer::{address::AddressParams, get, require_explorer},
    server::{EthData, error::ToolError},
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SourceParams {
    /// A name from `network_list` whose `capabilities.explorer` is true.
    pub network: String,
    /// The contract address.
    pub address: String,
    /// Include the full source text. Off by default because a verified
    /// contract's sources can be hundreds of kilobytes.
    pub include_source: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AbiResult {
    pub network: String,
    pub address: String,
    pub verified: bool,
    /// Absent when the contract is not verified. That is not the same as a
    /// contract with no functions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abi: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Set when this address is a proxy. The ABI above is the proxy's own;
    /// the implementation's is at this address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implementation: Option<String>,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SourceResult {
    pub network: String,
    pub address: String,
    pub verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compiler_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evm_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimization_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimization_runs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implementation: Option<String>,
    /// File name to source text. Present only when `include_source` was set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Value>,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

const UNVERIFIED: &str = "This contract's source has not been verified with this explorer, so no ABI is \
     published for it. Its bytecode is still readable with code_get, and you can still \
     call it with `call` if you know the signature.";

fn is_verified(body: &Value) -> bool {
    body.get("is_verified")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| body.get("abi").is_some_and(|abi| !abi.is_null()))
}

fn implementation_of(body: &Value) -> Option<String> {
    if let Some(address) = body.get("implementation_address").and_then(Value::as_str) {
        return Some(address.to_string());
    }
    body.get("implementations")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("address").or_else(|| item.get("address_hash")))
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[tool_router(router = contract_router, vis = "pub(crate)")]
impl EthData {
    /// Read a verified contract's ABI.
    #[tool(name = "contract_abi")]
    pub async fn contract_abi(
        &self,
        Parameters(params): Parameters<AddressParams>,
    ) -> Result<Json<AbiResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let base = require_explorer(&ready, &params.network)?;
        let address = self.address_of(&params.network, &params.address).await?;

        let body = get(base, &format!("/smart-contracts/{address}"), &[]).await?;
        let verified = is_verified(&body);

        Ok(Json(AbiResult {
            network: params.network,
            address: address.to_string(),
            verified,
            abi: verified.then(|| body.get("abi").cloned()).flatten(),
            name: body.get("name").and_then(Value::as_str).map(str::to_string),
            implementation: implementation_of(&body),
            source: base.to_string(),
            note: (!verified).then(|| UNVERIFIED.to_string()),
        }))
    }

    /// Read a verified contract's source metadata, and optionally its source.
    #[tool(name = "contract_source")]
    pub async fn contract_source(
        &self,
        Parameters(params): Parameters<SourceParams>,
    ) -> Result<Json<SourceResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let base = require_explorer(&ready, &params.network)?;
        let address = self.address_of(&params.network, &params.address).await?;

        let body = get(base, &format!("/smart-contracts/{address}"), &[]).await?;
        let verified = is_verified(&body);

        let files = if verified && params.include_source.unwrap_or(false) {
            let mut collected = serde_json::Map::new();
            if let Some(main) = body.get("source_code").and_then(Value::as_str) {
                let name = body
                    .get("file_path")
                    .and_then(Value::as_str)
                    .unwrap_or("main.sol");
                collected.insert(name.to_string(), Value::String(main.to_string()));
            }
            if let Some(extra) = body.get("additional_sources").and_then(Value::as_array) {
                for entry in extra {
                    let (Some(path), Some(text)) = (
                        entry.get("file_path").and_then(Value::as_str),
                        entry.get("source_code").and_then(Value::as_str),
                    ) else {
                        continue;
                    };
                    collected.insert(path.to_string(), Value::String(text.to_string()));
                }
            }
            (!collected.is_empty()).then_some(Value::Object(collected))
        } else {
            None
        };

        Ok(Json(SourceResult {
            network: params.network,
            address: address.to_string(),
            verified,
            name: body.get("name").and_then(Value::as_str).map(str::to_string),
            compiler_version: body
                .get("compiler_version")
                .and_then(Value::as_str)
                .map(str::to_string),
            evm_version: body
                .get("evm_version")
                .and_then(Value::as_str)
                .map(str::to_string),
            optimization_enabled: body.get("optimization_enabled").and_then(Value::as_bool),
            optimization_runs: body.get("optimization_runs").and_then(Value::as_u64),
            license: body
                .get("license_type")
                .and_then(Value::as_str)
                .map(str::to_string),
            implementation: implementation_of(&body),
            files,
            source: base.to_string(),
            note: (!verified).then(|| UNVERIFIED.to_string()),
        }))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn an_explicit_flag_decides_verification() {
        assert!(is_verified(&json!({ "is_verified": true })));
        assert!(!is_verified(&json!({ "is_verified": false })));
    }

    #[test]
    fn a_published_abi_implies_verification_when_the_flag_is_missing() {
        assert!(is_verified(&json!({ "abi": [] })));
        assert!(!is_verified(&json!({ "abi": null })));
        assert!(!is_verified(&json!({})));
    }

    #[test]
    fn a_flag_of_false_beats_a_present_abi() {
        assert!(!is_verified(&json!({ "is_verified": false, "abi": [] })));
    }

    #[test]
    fn finds_a_proxy_implementation_in_either_shape() {
        assert_eq!(
            implementation_of(&json!({ "implementation_address": "0x1" })).as_deref(),
            Some("0x1")
        );
        assert_eq!(
            implementation_of(&json!({ "implementations": [{ "address": "0x2" }] })).as_deref(),
            Some("0x2")
        );
        assert!(implementation_of(&json!({})).is_none());
    }
}
