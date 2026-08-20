//! `avatar_get`: the schema, ahead of the resolver.
//!
//! This feature is off by default and the tool does not resolve anything yet.
//! `eth-avatars` is published at 0.0.1 with `pub fn hello()` and nothing else,
//! so there is no resolver to call.
//!
//! The schema is written now, and only the schema, because where the resolution
//! logic lives is an open decision: an `ipfs://` record needs a gateway choice,
//! an `eip155:` record needs an on-chain `tokenURI` read, a `data:` record needs
//! a size policy, and all three need an SSRF policy because an avatar record is
//! attacker-controlled text naming a URL. Implementing that here and moving it
//! into `eth-avatars` afterwards would be the same work twice.
//!
//! Until then `ens_records` returns the raw `avatar` string, which is honest
//! and useful on its own.

use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::server::{EthData, error::ToolError};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AvatarParams {
    /// A name from `network_list` whose endpoint serves Ethereum mainnet.
    pub network: String,
    /// An ENS name, or an address to reverse-resolve first.
    pub name_or_address: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AvatarResult {
    pub network: String,
    /// The name the avatar record was read from.
    pub name: String,
    /// The record exactly as stored, which may be `ipfs://`, `https://`,
    /// `data:`, or an `eip155:` NFT reference.
    pub record: String,
    /// The record resolved to something fetchable over HTTPS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// How the record was resolved: `direct`, `ipfs_gateway`, or `nft`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_via: Option<String>,
}

#[tool_router(router = avatar_router, vis = "pub(crate)")]
impl EthData {
    /// Resolve an ENS avatar record to a fetchable URL.
    ///
    /// Not implemented yet. The schema is fixed; the resolver is waiting on
    /// `eth-avatars` to expose one.
    #[tool(name = "avatar_get")]
    pub async fn avatar_get(
        &self,
        Parameters(params): Parameters<AvatarParams>,
    ) -> Result<Json<AvatarResult>, ToolError> {
        Err(ToolError::unsupported(format!(
            "avatar_get is not implemented, so {:?} on {:?} cannot be resolved. \
             eth-avatars has no resolver yet.",
            params.name_or_address, params.network,
        ))
        .with_remedy("Use ens_records to read the raw `avatar` text record.".to_string()))
    }
}
