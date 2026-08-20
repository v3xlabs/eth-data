//! `token_get` and `token_holders`.
//!
//! Metadata prefers the contract over the index, because `symbol`, `decimals`,
//! and `totalSupply` are all one call away and the index can be behind.

use alloy::{primitives::U256, providers::Provider, sol};
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    explorer::{Page, address::AddressPageParams, get, page, require_explorer},
    server::{
        EthData,
        error::ToolError,
        output::{Amount, BlockContext, Source},
    },
};

sol! {
    #[sol(rpc)]
    interface IERC20Metadata {
        function name() external view returns (string);
        function symbol() external view returns (string);
        function decimals() external view returns (uint8);
        function totalSupply() external view returns (uint256);
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TokenParams {
    /// A name from `network_list`.
    pub network: String,
    /// The token contract address.
    pub address: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TokenResult {
    pub network: String,
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_supply: Option<Amount>,
    /// Where the fields above came from.
    pub source: Source,
    /// The block a chain read was taken at.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_at: Option<BlockContext>,
    /// The explorer's record, when one exists. It carries things the contract
    /// does not: holder counts, market data, an icon URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explorer_record: Option<Value>,
    #[cfg(feature = "icons")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub icons: Vec<crate::assets::icons::IconCandidate>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct HoldersResult {
    pub network: String,
    pub address: String,
    #[serde(flatten)]
    pub page: Page,
}

#[tool_router(router = token_router, vis = "pub(crate)")]
impl EthData {
    /// Read an ERC-20's metadata, from the contract first and the explorer
    /// second.
    #[tool(name = "token_get")]
    pub async fn token_get(
        &self,
        Parameters(params): Parameters<TokenParams>,
    ) -> Result<Json<TokenResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let address = crate::limits::parse_address(&params.address)?;

        let token = IERC20Metadata::new(address, &ready.provider);
        // EthCall is IntoFuture rather than Future, so each needs its own block.
        let (block, name, symbol, decimals, supply) = futures::join!(
            ready.provider.get_block_number(),
            async { token.name().call().await },
            async { token.symbol().call().await },
            async { token.decimals().call().await },
            async { token.totalSupply().call().await },
        );

        let on_chain = symbol.is_ok() || decimals.is_ok();
        let decimals = decimals.ok();
        let symbol = symbol.ok();

        // The explorer fills in what a contract cannot report about itself, and
        // is the fallback for a token with a non-standard metadata interface.
        let explorer_record = match require_explorer(&ready, &params.network) {
            Ok(base) => get(base, &format!("/tokens/{address}"), &[]).await.ok(),
            Err(_) => None,
        };

        let indexed = |key: &str| -> Option<String> {
            explorer_record
                .as_ref()?
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
        };

        let decimals = decimals.or_else(|| indexed("decimals")?.parse().ok());
        let total_supply = supply
            .ok()
            .or_else(|| U256::from_str_radix(&indexed("total_supply")?, 10).ok());

        Ok(Json(TokenResult {
            network: params.network,
            address: address.to_string(),
            name: name.ok().or_else(|| indexed("name")),
            total_supply: total_supply
                .map(|raw| Amount::new(raw, decimals.unwrap_or(0), symbol.clone())),
            symbol: symbol.or_else(|| indexed("symbol")),
            decimals,
            source: if on_chain {
                Source::OnChain
            } else {
                Source::Explorer
            },
            read_at: block.ok().map(BlockContext::number),
            #[cfg(feature = "icons")]
            icons: crate::assets::icons::erc20_candidates(
                &self.state.icons,
                ready.chain_id,
                address,
            ),
            explorer_record,
        }))
    }

    /// One bounded page of a token's holders, largest first.
    #[tool(name = "token_holders")]
    pub async fn token_holders(
        &self,
        Parameters(params): Parameters<AddressPageParams>,
    ) -> Result<Json<HoldersResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let address = crate::limits::parse_address(&params.address)?;

        let page = page(
            &ready,
            &params.network,
            &format!("/tokens/{address}/holders"),
            params.page_size,
            params.cursor.as_deref(),
            &[],
        )
        .await?;

        Ok(Json(HoldersResult {
            network: params.network,
            address: address.to_string(),
            page,
        }))
    }
}
