//! `call` and `gas_estimate`. Neither can change state, and neither signs.

use alloy::{
    primitives::{Address, Bytes, U256},
    providers::Provider,
    rpc::types::TransactionRequest,
};
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    chain::{block_id, native_symbol, rpc_error},
    limits::{parse_address, parse_bytes, parse_u256},
    server::{EthData, error::ToolError, output::Amount},
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CallParams {
    /// A name from `network_list`.
    pub network: String,
    /// The contract to call.
    pub to: String,
    /// ABI-encoded calldata. Build it with `calldata_encode`. Defaults to `0x`.
    pub data: Option<String>,
    /// The caller to simulate as. Defaults to the zero address.
    pub from: Option<String>,
    /// Native value to attach, decimal or 0x-prefixed wei.
    pub value: Option<String>,
    /// Block number, tag, or hash. Defaults to `latest`.
    pub block: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GasEstimateParams {
    /// A name from `network_list`.
    pub network: String,
    /// The contract to call. Omit to estimate a deployment.
    pub to: Option<String>,
    /// ABI-encoded calldata, or deployment bytecode when `to` is omitted.
    pub data: Option<String>,
    /// The sender to simulate as.
    pub from: Option<String>,
    /// Native value to attach, decimal or 0x-prefixed wei.
    pub value: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CallResult {
    pub network: String,
    /// Raw return data. Decode it with `abi_decode`.
    pub data: String,
    pub size: usize,
    /// True when the call returned nothing, which for a contract that exists
    /// usually means the function does not.
    pub empty: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GasEstimateResult {
    pub network: String,
    pub gas: u64,
    /// Gas multiplied by the current gas price, as a spend estimate.
    pub estimated_fee: Amount,
}

fn request(
    to: Option<Address>,
    data: Option<&str>,
    from: Option<&str>,
    value: Option<&str>,
) -> Result<TransactionRequest, ToolError> {
    let mut request = TransactionRequest::default();
    if let Some(to) = to {
        request = request.to(to);
    }
    if let Some(data) = data {
        request.input = parse_bytes(data, "data")?.into();
    } else {
        request.input = Bytes::new().into();
    }
    if let Some(from) = from {
        request = request.from(parse_address(from)?);
    }
    if let Some(value) = value {
        request = request.value(parse_u256(value, "value")?);
    }
    Ok(request)
}

#[tool_router(router = call_router, vis = "pub(crate)")]
impl EthData {
    /// Execute a read-only call. This never broadcasts and never changes state,
    /// on this or any other network.
    #[tool(name = "call")]
    pub async fn call(
        &self,
        Parameters(params): Parameters<CallParams>,
    ) -> Result<Json<CallResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let to = parse_address(&params.to)?;
        let id = block_id(params.block.as_deref())?;
        let request = request(
            Some(to),
            params.data.as_deref(),
            params.from.as_deref(),
            params.value.as_deref(),
        )?;

        let data = ready
            .provider
            .call(request)
            .block(id)
            .await
            .map_err(|error| rpc_error("eth_call", error))?;

        Ok(Json(CallResult {
            network: params.network,
            data: data.to_string(),
            size: data.len(),
            empty: data.is_empty(),
        }))
    }

    /// Estimate the gas a transaction would use, without broadcasting it.
    #[tool(name = "gas_estimate")]
    pub async fn gas_estimate(
        &self,
        Parameters(params): Parameters<GasEstimateParams>,
    ) -> Result<Json<GasEstimateResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let to = params.to.as_deref().map(parse_address).transpose()?;
        let request = request(
            to,
            params.data.as_deref(),
            params.from.as_deref(),
            params.value.as_deref(),
        )?;

        let gas = ready
            .provider
            .estimate_gas(request)
            .await
            .map_err(|error| rpc_error("eth_estimateGas", error))?;

        let gas_price = ready.provider.get_gas_price().await.unwrap_or_default();
        let fee = U256::from(gas) * U256::from(gas_price);

        Ok(Json(GasEstimateResult {
            network: params.network,
            gas,
            estimated_fee: Amount::native(fee, native_symbol(&ready)),
        }))
    }
}
