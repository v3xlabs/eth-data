//! `nft_get` and `nft_transfers`.

use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    explorer::{Page, get, page, require_explorer},
    server::{EthData, error::ToolError},
};

const MAX_TOKEN_ID_BYTES: usize = 78;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NftParams {
    /// A name from `network_list` whose `capabilities.explorer` is true.
    pub network: String,
    /// The collection contract address.
    pub contract: String,
    /// The token id, as a decimal string.
    pub token_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NftTransfersParams {
    /// A name from `network_list` whose `capabilities.explorer` is true.
    pub network: String,
    /// The collection contract address.
    pub contract: String,
    /// Restrict to one token id. Omit for the whole collection.
    pub token_id: Option<String>,
    /// Items per page, 1 to 100. Defaults to 50.
    pub page_size: Option<u32>,
    /// A `next_cursor` from a previous call.
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct NftResult {
    pub network: String,
    pub contract: String,
    pub token_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// The raw `tokenURI`. An `ipfs://` value is not fetchable as written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_uri: Option<String>,
    /// True when `metadata_uri` needs an IPFS gateway to become a URL.
    pub metadata_uri_is_ipfs: bool,
    pub source: String,
    /// The explorer's record, including any metadata it already resolved.
    pub item: Value,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct NftTransfersResult {
    pub network: String,
    pub contract: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_id: Option<String>,
    #[serde(flatten)]
    pub page: Page,
}

/// Token ids are `uint256`, so they are decimal strings rather than numbers.
/// They are also part of a URL path, which is why they are checked rather than
/// interpolated as given.
fn check_token_id(token_id: &str) -> Result<&str, ToolError> {
    if token_id.is_empty()
        || token_id.len() > MAX_TOKEN_ID_BYTES
        || !token_id.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(ToolError::invalid_input(
            "token_id must be a decimal integer.".to_string(),
        ));
    }
    Ok(token_id)
}

#[tool_router(router = nft_router, vis = "pub(crate)")]
impl EthData {
    /// Read one NFT: its owner, its metadata URI, and whatever metadata the
    /// explorer has already resolved.
    #[tool(name = "nft_get")]
    pub async fn nft_get(
        &self,
        Parameters(params): Parameters<NftParams>,
    ) -> Result<Json<NftResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let base = require_explorer(&ready, &params.network)?;
        let contract = crate::limits::parse_address(&params.contract)?;
        let token_id = check_token_id(&params.token_id)?;

        let item = get(
            base,
            &format!("/tokens/{contract}/instances/{token_id}"),
            &[],
        )
        .await?;

        let metadata_uri = item
            .get("metadata_url")
            .or_else(|| item.get("image_url"))
            .and_then(Value::as_str)
            .map(str::to_string);

        Ok(Json(NftResult {
            network: params.network,
            contract: contract.to_string(),
            token_id: token_id.to_string(),
            owner: item
                .get("owner")
                .and_then(|owner| owner.get("hash").or(Some(owner)))
                .and_then(Value::as_str)
                .map(str::to_string),
            metadata_uri_is_ipfs: metadata_uri
                .as_deref()
                .is_some_and(|uri| uri.starts_with("ipfs://")),
            metadata_uri,
            source: base.to_string(),
            item,
        }))
    }

    /// One bounded page of transfers for a collection, or for one token in it.
    #[tool(name = "nft_transfers")]
    pub async fn nft_transfers(
        &self,
        Parameters(params): Parameters<NftTransfersParams>,
    ) -> Result<Json<NftTransfersResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let contract = crate::limits::parse_address(&params.contract)?;

        let path = match &params.token_id {
            Some(token_id) => {
                let token_id = check_token_id(token_id)?;
                format!("/tokens/{contract}/instances/{token_id}/transfers")
            }
            None => format!("/tokens/{contract}/transfers"),
        };

        let page = page(
            &ready,
            &params.network,
            &path,
            params.page_size,
            params.cursor.as_deref(),
            &[],
        )
        .await?;

        Ok(Json(NftTransfersResult {
            network: params.network,
            contract: contract.to_string(),
            token_id: params.token_id,
            page,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_decimal_id() {
        assert!(check_token_id("0").is_ok());
        assert!(
            check_token_id(
                "115792089237316195423570985008687907853269984665640564039457584007913129639935"
            )
            .is_ok()
        );
    }

    #[test]
    fn rejects_anything_that_could_change_the_path() {
        assert!(check_token_id("1/../../addresses").is_err());
        assert!(check_token_id("0x01").is_err());
        assert!(check_token_id("").is_err());
        assert!(check_token_id("1 ").is_err());
    }

    #[test]
    fn rejects_an_id_longer_than_a_uint256() {
        assert!(check_token_id(&"9".repeat(MAX_TOKEN_ID_BYTES + 1)).is_err());
    }
}
