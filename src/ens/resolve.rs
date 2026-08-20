//! `ens_resolve` and `ens_reverse`.

use alloy::{ens::ProviderEnsExt, primitives::Address};
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    config::network::Ready,
    ens::{check_name, ens_error, normalize, require_ens},
    limits::parse_address,
    server::{EthData, error::ToolError},
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ResolveParams {
    /// A name from `network_list` whose endpoint serves Ethereum mainnet.
    pub network: String,
    /// An ENS name, for example `vitalik.eth`.
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReverseParams {
    /// A name from `network_list` whose endpoint serves Ethereum mainnet.
    pub network: String,
    /// A 20-byte address.
    pub address: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ResolveResult {
    pub network: String,
    pub name: String,
    pub address: String,
    /// False when the name contains characters this build cannot normalize, in
    /// which case it was hashed exactly as given.
    pub fully_normalized: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReverseResult {
    pub network: String,
    pub address: String,
    /// The reverse record, if one is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// True only when resolving `name` forward returns `address` again.
    ///
    /// A reverse record is written by whoever controls the address and is not
    /// proof of anything on its own. An unverified name is somebody's claim,
    /// not an identity, so do not display it as one.
    pub forward_verified: bool,
    /// Where forward verification landed, when it disagreed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forward_resolves_to: Option<String>,
}

pub(crate) struct Forward {
    pub address: Address,
    pub fully_normalized: bool,
}

pub(crate) async fn forward(
    ready: &Ready,
    name: &crate::ens::Normalized,
) -> Result<Forward, ToolError> {
    check_name(name)?;
    let address = ready
        .provider
        .resolve_name(&name.name)
        .await
        .map_err(|error| match error.to_string() {
            text if text.contains("not found") => ToolError::upstream(format!(
                "{:?} has no resolver, so it does not resolve to an address.",
                name.name
            )),
            text => ens_error("ENS resolution", text),
        })?;

    if address.is_zero() {
        return Err(ToolError::upstream(format!(
            "{:?} resolves to the zero address, which means no address is set.",
            name.name
        )));
    }

    Ok(Forward {
        address,
        fully_normalized: name.fully_normalized,
    })
}

#[tool_router(router = ens_resolve_router, vis = "pub(crate)")]
impl EthData {
    /// Resolve an ENS name to an address.
    #[tool(name = "ens_resolve")]
    pub async fn ens_resolve(
        &self,
        Parameters(params): Parameters<ResolveParams>,
    ) -> Result<Json<ResolveResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        require_ens(&ready, &params.network)?;

        let name = normalize(&params.name);
        let resolved = forward(&ready, &name).await?;

        Ok(Json(ResolveResult {
            network: params.network,
            name: name.name,
            address: resolved.address.to_string(),
            fully_normalized: resolved.fully_normalized,
        }))
    }

    /// Resolve an address to its primary ENS name, and check that the name
    /// resolves back to the same address before reporting it as verified.
    #[tool(name = "ens_reverse")]
    pub async fn ens_reverse(
        &self,
        Parameters(params): Parameters<ReverseParams>,
    ) -> Result<Json<ReverseResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        require_ens(&ready, &params.network)?;
        let address = parse_address(&params.address)?;

        let name = match ready.provider.lookup_address(&address).await {
            Ok(name) if !name.is_empty() => name,
            // No reverse record is the common case, not a failure.
            _ => {
                return Ok(Json(ReverseResult {
                    network: params.network,
                    address: address.to_string(),
                    name: None,
                    forward_verified: false,
                    forward_resolves_to: None,
                }));
            }
        };

        let normalized = normalize(&name);
        let forward_address = forward(&ready, &normalized).await.ok().map(|f| f.address);
        let verified = forward_address == Some(address);

        Ok(Json(ReverseResult {
            network: params.network,
            address: address.to_string(),
            name: Some(normalized.name),
            forward_verified: verified,
            forward_resolves_to: (!verified)
                .then_some(forward_address)
                .flatten()
                .map(|found| found.to_string()),
        }))
    }
}
