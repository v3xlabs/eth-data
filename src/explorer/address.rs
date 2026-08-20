//! `address_get`, `address_txs`, `address_transfers`, `address_nfts`.

use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    explorer::{Page, get, page, require_explorer},
    server::{EthData, error::ToolError},
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddressParams {
    /// A name from `network_list` whose `capabilities.explorer` is true.
    pub network: String,
    /// An address, or an ENS name when the `ens` feature is compiled in.
    pub address: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddressPageParams {
    /// A name from `network_list` whose `capabilities.explorer` is true.
    pub network: String,
    /// An address, or an ENS name when the `ens` feature is compiled in.
    pub address: String,
    /// Items per page, 1 to 100. Defaults to 50.
    pub page_size: Option<u32>,
    /// A `next_cursor` from a previous call.
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TransfersParams {
    /// A name from `network_list` whose `capabilities.explorer` is true.
    pub network: String,
    /// An address, or an ENS name when the `ens` feature is compiled in.
    pub address: String,
    /// Restrict to transfers of this token contract.
    pub token: Option<String>,
    /// Items per page, 1 to 100. Defaults to 50.
    pub page_size: Option<u32>,
    /// A `next_cursor` from a previous call.
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AddressResult {
    pub network: String,
    pub address: String,
    pub is_contract: bool,
    pub is_verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub source: String,
    /// The explorer's own record, unaltered.
    pub overview: Value,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AddressPage {
    pub network: String,
    pub address: String,
    #[serde(flatten)]
    pub page: Page,
}

#[tool_router(router = address_router, vis = "pub(crate)")]
impl EthData {
    /// Read what the explorer knows about an address: whether it is a contract,
    /// whether its source is verified, its tags, and its balance.
    ///
    /// This is indexed data. It is good for orientation and should not be the
    /// basis of a decision that a chain read could settle.
    #[tool(name = "address_get")]
    pub async fn address_get(
        &self,
        Parameters(params): Parameters<AddressParams>,
    ) -> Result<Json<AddressResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let base = require_explorer(&ready, &params.network)?;
        let address = self.address_of(&params.network, &params.address).await?;

        let overview = get(base, &format!("/addresses/{address}"), &[]).await?;

        Ok(Json(AddressResult {
            network: params.network,
            address: address.to_string(),
            is_contract: overview
                .get("is_contract")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            is_verified: overview
                .get("is_verified")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            name: overview
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string),
            source: base.to_string(),
            overview,
        }))
    }

    /// One bounded page of an address's transactions, newest first.
    #[tool(name = "address_txs")]
    pub async fn address_txs(
        &self,
        Parameters(params): Parameters<AddressPageParams>,
    ) -> Result<Json<AddressPage>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let address = self.address_of(&params.network, &params.address).await?;

        let page = page(
            &ready,
            &params.network,
            &format!("/addresses/{address}/transactions"),
            params.page_size,
            params.cursor.as_deref(),
            &[],
        )
        .await?;

        Ok(Json(AddressPage {
            network: params.network,
            address: address.to_string(),
            page,
        }))
    }

    /// One bounded page of an address's token transfers, newest first.
    #[tool(name = "address_transfers")]
    pub async fn address_transfers(
        &self,
        Parameters(params): Parameters<TransfersParams>,
    ) -> Result<Json<AddressPage>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let address = self.address_of(&params.network, &params.address).await?;

        let token = match &params.token {
            Some(token) => Some(crate::limits::parse_address(token)?.to_string()),
            None => None,
        };
        let extra: Vec<(&str, &str)> = match &token {
            Some(token) => vec![("token", token.as_str())],
            None => Vec::new(),
        };

        let page = page(
            &ready,
            &params.network,
            &format!("/addresses/{address}/token-transfers"),
            params.page_size,
            params.cursor.as_deref(),
            &extra,
        )
        .await?;

        Ok(Json(AddressPage {
            network: params.network,
            address: address.to_string(),
            page,
        }))
    }

    /// One bounded page of the NFTs an address holds.
    #[tool(name = "address_nfts")]
    pub async fn address_nfts(
        &self,
        Parameters(params): Parameters<AddressPageParams>,
    ) -> Result<Json<AddressPage>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let address = self.address_of(&params.network, &params.address).await?;

        let page = page(
            &ready,
            &params.network,
            &format!("/addresses/{address}/nft"),
            params.page_size,
            params.cursor.as_deref(),
            &[("type", "ERC-721,ERC-1155,ERC-404")],
        )
        .await?;

        Ok(Json(AddressPage {
            network: params.network,
            address: address.to_string(),
            page,
        }))
    }
}
