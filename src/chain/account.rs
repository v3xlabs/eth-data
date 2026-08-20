//! `balance_get`, `nonce_get`, `code_get`, `storage_get`.

use alloy::providers::Provider;
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    chain::{block_id, native_symbol, rpc_error},
    limits::{parse_address, parse_u256},
    server::{
        EthData,
        error::ToolError,
        output::{Amount, BlockContext},
    },
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BalanceParams {
    /// A name from `network_list`.
    pub network: String,
    /// A 20-byte address, or an ENS name when the `ens` feature is compiled in.
    pub address: String,
    /// Block number, tag, or hash. Defaults to `latest`.
    pub block: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AccountParams {
    /// A name from `network_list`.
    pub network: String,
    /// A 20-byte address.
    pub address: String,
    /// Block number, tag, or hash. Defaults to `latest`.
    pub block: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StorageParams {
    /// A name from `network_list`.
    pub network: String,
    /// A 20-byte contract address.
    pub address: String,
    /// Slot index, decimal or 0x-prefixed.
    pub slot: String,
    /// Block number, tag, or hash. Defaults to `latest`.
    pub block: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BalanceResult {
    pub network: String,
    pub address: String,
    pub balance: Amount,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<BlockContext>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct NonceResult {
    pub network: String,
    pub address: String,
    /// Transactions sent from this address, which is also the nonce the next
    /// one would use.
    pub nonce: u64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CodeResult {
    pub network: String,
    pub address: String,
    /// False for an externally owned account, and for a contract that has not
    /// been deployed at this block yet.
    pub is_contract: bool,
    pub size: usize,
    pub code: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct StorageResult {
    pub network: String,
    pub address: String,
    pub slot: String,
    /// The raw 32-byte word.
    pub value: String,
    /// The same word read as an unsigned integer, which is what a `uint256`,
    /// a balance, or a packed small value usually is.
    pub as_uint: String,
    /// The low 20 bytes, which is what the word holds when the slot is an
    /// `address`. Meaningless for other layouts.
    pub as_address: String,
}

#[tool_router(router = account_router, vis = "pub(crate)")]
impl EthData {
    /// Read a native balance for an address or ENS name.
    #[tool(name = "balance_get")]
    pub async fn balance_get(
        &self,
        Parameters(params): Parameters<BalanceParams>,
    ) -> Result<Json<BalanceResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let address = self.address_of(&params.network, &params.address).await?;
        let id = block_id(params.block.as_deref())?;

        let balance = ready
            .provider
            .get_balance(address)
            .block_id(id)
            .await
            .map_err(|error| rpc_error("eth_getBalance", error))?;

        Ok(Json(BalanceResult {
            network: params.network,
            address: address.to_string(),
            balance: Amount::native(balance, native_symbol(&ready)),
            block: None,
        }))
    }

    /// Read how many transactions an address has sent.
    #[tool(name = "nonce_get")]
    pub async fn nonce_get(
        &self,
        Parameters(params): Parameters<AccountParams>,
    ) -> Result<Json<NonceResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let address = parse_address(&params.address)?;
        let id = block_id(params.block.as_deref())?;

        let nonce = ready
            .provider
            .get_transaction_count(address)
            .block_id(id)
            .await
            .map_err(|error| rpc_error("eth_getTransactionCount", error))?;

        Ok(Json(NonceResult {
            network: params.network,
            address: address.to_string(),
            nonce,
        }))
    }

    /// Read deployed runtime bytecode.
    #[tool(name = "code_get")]
    pub async fn code_get(
        &self,
        Parameters(params): Parameters<AccountParams>,
    ) -> Result<Json<CodeResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let address = parse_address(&params.address)?;
        let id = block_id(params.block.as_deref())?;

        let code = ready
            .provider
            .get_code_at(address)
            .block_id(id)
            .await
            .map_err(|error| rpc_error("eth_getCode", error))?;

        Ok(Json(CodeResult {
            network: params.network,
            address: address.to_string(),
            is_contract: !code.is_empty(),
            size: code.len(),
            code: code.to_string(),
        }))
    }

    /// Read one contract storage slot, and the two readings of that word that
    /// are almost always what the caller wanted.
    #[tool(name = "storage_get")]
    pub async fn storage_get(
        &self,
        Parameters(params): Parameters<StorageParams>,
    ) -> Result<Json<StorageResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let address = parse_address(&params.address)?;
        let slot = parse_u256(&params.slot, "slot")?;
        let id = block_id(params.block.as_deref())?;

        let value = ready
            .provider
            .get_storage_at(address, slot)
            .block_id(id)
            .await
            .map_err(|error| rpc_error("eth_getStorageAt", error))?;

        let word = alloy::primitives::B256::from(value);
        let as_address = alloy::primitives::Address::from_word(word);

        Ok(Json(StorageResult {
            network: params.network,
            address: address.to_string(),
            slot: slot.to_string(),
            value: word.to_string(),
            as_uint: value.to_string(),
            as_address: as_address.to_string(),
        }))
    }
}
