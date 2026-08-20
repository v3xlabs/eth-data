//! `icon_get`: ranked icon candidates from every configured source.
//!
//! `eth_icons::IconClient::resolve` fetches image bytes. This does not call it.
//! It walks `sources()` calling `IconSource::url`, which builds the candidate
//! URL without a request, so the tool makes no outbound traffic at all and the
//! caller gets every option rather than the first one that happened to load.

use alloy::primitives::Address;
use eth_icons::{IconClient, IconQuery};
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    limits::parse_address,
    server::{EthData, error::ToolError},
};

pub fn client() -> IconClient {
    IconClient::builder().with_defaults().build()
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IconKind {
    /// The chain's own logo.
    Network,
    /// The chain's native asset, such as ETH.
    Native,
    /// An ERC-20, identified by `token`.
    Erc20,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IconParams {
    /// A name from `network_list`. Its chain ID selects the icon set.
    pub network: String,
    /// What to fetch an icon for. Defaults to `erc20` when `token` is given,
    /// and `network` when it is not.
    pub kind: Option<IconKind>,
    /// The ERC-20 contract address. Required when `kind` is `erc20`.
    pub token: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct IconCandidate {
    /// The source that offers this URL: blockscout, avara, zerion, smoldapp,
    /// or safewallet.
    pub source: String,
    pub url: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IconResult {
    pub network: String,
    pub chain_id: u64,
    pub kind: IconKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Ranked by source order. These are candidates, not confirmations: no
    /// request was made, so a URL here may still 404.
    pub candidates: Vec<IconCandidate>,
    /// Sources that do not cover this kind of query, named rather than
    /// silently dropped.
    pub unsupported_sources: Vec<String>,
}

fn candidates(client: &IconClient, query: &IconQuery) -> (Vec<IconCandidate>, Vec<String>) {
    let mut found = Vec::new();
    let mut unsupported = Vec::new();
    for source in client.sources() {
        match source.url(query) {
            Some(url) => found.push(IconCandidate {
                source: source.name().to_string(),
                url,
            }),
            None => unsupported.push(source.name().to_string()),
        }
    }
    (found, unsupported)
}

/// Used by `address_tokens` and `token_get` to hang icons off a token row.
#[cfg_attr(not(feature = "explorer"), allow(dead_code))]
pub fn erc20_candidates(client: &IconClient, chain_id: u64, token: Address) -> Vec<IconCandidate> {
    candidates(client, &IconQuery::ERC20(chain_id, token.to_string())).0
}

#[tool_router(router = icon_router, vis = "pub(crate)")]
impl EthData {
    /// Resolve icon candidate URLs for a network, its native asset, or an
    /// ERC-20.
    ///
    /// Returns URLs and their sources. It does not download any image, so the
    /// call is free and no image bytes enter the conversation.
    #[tool(name = "icon_get")]
    pub async fn icon_get(
        &self,
        Parameters(params): Parameters<IconParams>,
    ) -> Result<Json<IconResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;

        let kind = params.kind.unwrap_or(if params.token.is_some() {
            IconKind::Erc20
        } else {
            IconKind::Network
        });

        let token = match (kind, &params.token) {
            (IconKind::Erc20, Some(token)) => Some(parse_address(token)?),
            (IconKind::Erc20, None) => {
                return Err(ToolError::invalid_input(
                    "kind `erc20` needs a `token` address.".to_string(),
                ));
            }
            _ => None,
        };

        let query = match (kind, token) {
            (IconKind::Network, _) => IconQuery::Network(ready.chain_id),
            (IconKind::Native, _) => IconQuery::Native(ready.chain_id),
            (IconKind::Erc20, Some(token)) => IconQuery::ERC20(ready.chain_id, token.to_string()),
            (IconKind::Erc20, None) => unreachable!("guarded above"),
        };

        let (candidates, unsupported_sources) = candidates(&self.state.icons, &query);

        Ok(Json(IconResult {
            network: params.network,
            chain_id: ready.chain_id,
            kind,
            token: token.map(|token| token.to_string()),
            candidates,
            unsupported_sources,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_source_is_accounted_for() {
        let client = client();
        let total = client.sources().len();
        let (found, unsupported) = candidates(
            &client,
            &IconQuery::ERC20(1, "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_string()),
        );
        assert_eq!(found.len() + unsupported.len(), total);
        assert!(!found.is_empty(), "no source covers a mainnet ERC-20");
    }

    #[test]
    fn candidates_are_urls_and_nothing_was_fetched() {
        let client = client();
        let (found, _) = candidates(&client, &IconQuery::Network(1));
        for candidate in &found {
            assert!(
                candidate.url.starts_with("https://"),
                "{} offered {}",
                candidate.source,
                candidate.url
            );
        }
    }

    #[test]
    fn an_unknown_chain_still_answers_rather_than_failing() {
        let client = client();
        let (found, unsupported) = candidates(&client, &IconQuery::Native(999_999_999));
        assert_eq!(found.len() + unsupported.len(), client.sources().len());
    }
}
