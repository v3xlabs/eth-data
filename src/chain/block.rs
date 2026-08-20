//! `block_get`, `tx_get`, `receipt_get`.
//!
//! These three return the canonical RPC object as the protocol defines it, hex
//! quantities and all, plus a `summary` holding the handful of fields that are
//! most often misread out of hex.

use alloy::{consensus::Transaction as _, network::TransactionResponse as _, providers::Provider};
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    chain::{block_id, native_symbol, rpc_error},
    limits::parse_hash,
    server::{EthData, error::ToolError, output::Amount},
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BlockGetParams {
    /// A name from `network_list`.
    pub network: String,
    /// Block number, a tag (`latest`, `earliest`, `pending`, `safe`,
    /// `finalized`), or a 32-byte block hash. Defaults to `latest`.
    pub block: Option<String>,
    /// Return full transaction objects instead of hashes. Defaults to false,
    /// because a full mainnet block is large.
    pub include_transactions: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TxParams {
    /// A name from `network_list`.
    pub network: String,
    /// 32-byte transaction hash.
    pub transaction_hash: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BlockResult {
    pub network: String,
    pub number: u64,
    pub hash: String,
    pub timestamp: u64,
    pub transaction_count: usize,
    pub block: Value,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TransactionResult {
    pub network: String,
    pub found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<TransactionSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<Value>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TransactionSummary {
    pub from: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// Absent while the transaction is still pending.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_number: Option<u64>,
    pub nonce: u64,
    pub value: Amount,
    /// True when `to` is empty, which means this deploys a contract.
    pub is_deployment: bool,
    /// The 4-byte selector, when there is calldata to take one from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    pub input_size: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReceiptResult {
    pub network: String,
    pub found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<ReceiptSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<Value>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReceiptSummary {
    pub succeeded: bool,
    pub block_number: u64,
    pub gas_used: u64,
    pub fee_paid: Amount,
    pub log_count: usize,
    /// Set when this transaction deployed a contract.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract_address: Option<String>,
}

#[tool_router(router = block_router, vis = "pub(crate)")]
impl EthData {
    /// Read one block by number, tag, or hash.
    #[tool(name = "block_get")]
    pub async fn block_get(
        &self,
        Parameters(params): Parameters<BlockGetParams>,
    ) -> Result<Json<BlockResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let id = block_id(params.block.as_deref())?;

        let request = ready.provider.get_block(id);
        let block = if params.include_transactions.unwrap_or(false) {
            request.full().await
        } else {
            request.hashes().await
        }
        .map_err(|error| rpc_error("eth_getBlock", error))?
        .ok_or_else(|| {
            ToolError::upstream(format!("No block matched {:?}.", params.block.as_deref()))
        })?;

        let value = serde_json::to_value(&block)
            .map_err(|error| ToolError::upstream(format!("Malformed block: {error}")))?;

        Ok(Json(BlockResult {
            network: params.network,
            number: block.header.number,
            hash: block.header.hash.to_string(),
            timestamp: block.header.timestamp,
            transaction_count: block.transactions.len(),
            block: value,
        }))
    }

    /// Read one transaction by hash. A hash the node has never seen returns
    /// `found: false` rather than an error, because "not present" is an answer.
    #[tool(name = "tx_get")]
    pub async fn tx_get(
        &self,
        Parameters(params): Parameters<TxParams>,
    ) -> Result<Json<TransactionResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let hash = parse_hash(&params.transaction_hash, "transaction_hash")?;

        let transaction = ready
            .provider
            .get_transaction_by_hash(hash)
            .await
            .map_err(|error| rpc_error("eth_getTransactionByHash", error))?;

        let Some(transaction) = transaction else {
            return Ok(Json(TransactionResult {
                network: params.network,
                found: false,
                summary: None,
                transaction: None,
            }));
        };

        let input = transaction.input();
        let summary = TransactionSummary {
            from: transaction.from().to_string(),
            to: transaction.to().map(|to| to.to_string()),
            block_number: transaction.block_number,
            nonce: transaction.nonce(),
            value: Amount::native(transaction.value(), native_symbol(&ready)),
            is_deployment: transaction.to().is_none(),
            selector: (input.len() >= 4).then(|| format!("0x{}", alloy::hex::encode(&input[..4]))),
            input_size: input.len(),
        };

        let value = serde_json::to_value(&transaction)
            .map_err(|error| ToolError::upstream(format!("Malformed transaction: {error}")))?;

        Ok(Json(TransactionResult {
            network: params.network,
            found: true,
            summary: Some(summary),
            transaction: Some(value),
        }))
    }

    /// Read one transaction receipt by hash. `summary.succeeded` is the field
    /// worth reading first: a mined transaction can still have reverted.
    #[tool(name = "receipt_get")]
    pub async fn receipt_get(
        &self,
        Parameters(params): Parameters<TxParams>,
    ) -> Result<Json<ReceiptResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let hash = parse_hash(&params.transaction_hash, "transaction_hash")?;

        let receipt = ready
            .provider
            .get_transaction_receipt(hash)
            .await
            .map_err(|error| rpc_error("eth_getTransactionReceipt", error))?;

        let Some(receipt) = receipt else {
            return Ok(Json(ReceiptResult {
                network: params.network,
                found: false,
                summary: None,
                receipt: None,
            }));
        };

        let fee = alloy::primitives::U256::from(receipt.gas_used)
            * alloy::primitives::U256::from(receipt.effective_gas_price);

        let summary = ReceiptSummary {
            succeeded: receipt.status(),
            block_number: receipt.block_number.unwrap_or_default(),
            gas_used: receipt.gas_used,
            fee_paid: Amount::native(fee, native_symbol(&ready)),
            log_count: receipt.inner.logs().len(),
            contract_address: receipt.contract_address.map(|a| a.to_string()),
        };

        let value = serde_json::to_value(&receipt)
            .map_err(|error| ToolError::upstream(format!("Malformed receipt: {error}")))?;

        Ok(Json(ReceiptResult {
            network: params.network,
            found: true,
            summary: Some(summary),
            receipt: Some(value),
        }))
    }
}
