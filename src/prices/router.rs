//! Building and caching an `eth-prices` router per chain and asset.
//!
//! Discovery walks Uniswap v2 and v3 factories and ERC-4626 vaults, which is
//! tens of RPC calls. Doing it once per asset and keeping the result is the
//! difference between a price tool that answers in a second and one that does
//! not get used.

use std::{collections::HashMap, sync::Arc};

use alloy::primitives::U256;
use eth_prices::{
    asset::AssetIdentifier,
    quoter::fixed::FixedQuoter,
    router::{Router, auto::AutoRouter},
};
use tokio::sync::Mutex;

use crate::{
    config::{derived::Anchor, network::Ready},
    server::error::ToolError,
};

/// The fee tiers worth probing. Anything rarer costs a discovery call per
/// token pair for a pool that almost never exists.
const UNISWAP_V3_FEES: &[u32] = &[100, 500, 3000, 10000];

pub const USD: &str = "fiat:usd";

#[derive(Default)]
pub struct RouterCache {
    inner: Mutex<HashMap<(u64, String), Arc<Router>>>,
}

impl RouterCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get(
        &self,
        ready: &Ready,
        asset: &AssetIdentifier,
        quote: &AssetIdentifier,
    ) -> Result<Arc<Router>, ToolError> {
        let key = (ready.chain_id, format!("{asset}->{quote}"));
        if let Some(router) = self.inner.lock().await.get(&key) {
            return Ok(Arc::clone(router));
        }

        let router = Arc::new(build(ready, asset, quote).await?);
        self.inner.lock().await.insert(key, Arc::clone(&router));
        Ok(router)
    }
}

/// A chain with no anchors has nothing to route through. Testnet pools price
/// against play money, and a number derived from one would be worse than no
/// number at all.
pub fn anchors_of(ready: &Ready, network: &str) -> Result<&'static [Anchor], ToolError> {
    let anchors = ready.chain.map(|chain| chain.anchors).unwrap_or(&[]);
    if anchors.is_empty() {
        return Err(ToolError::unsupported(format!(
            "No price anchors are known for chain {} ({network:?}).",
            ready.chain_id
        ))
        .with_remedy(
            "Prices need a wrapped native asset and a stablecoin with real liquidity. \
             Testnets deliberately have none."
                .to_string(),
        ));
    }
    Ok(anchors)
}

async fn build(
    ready: &Ready,
    asset: &AssetIdentifier,
    quote: &AssetIdentifier,
) -> Result<Router, ToolError> {
    let anchors = ready.chain.map(|chain| chain.anchors).unwrap_or(&[]);

    let mut tokens: Vec<AssetIdentifier> = anchors
        .iter()
        .map(|anchor| AssetIdentifier::ERC20 {
            address: anchor.address,
        })
        .collect();
    for identifier in [asset, quote] {
        if matches!(identifier, AssetIdentifier::ERC20 { .. }) && !tokens.contains(identifier) {
            tokens.push(identifier.clone());
        }
    }

    let mut router = AutoRouter::new(ready.provider.clone(), tokens)
        .with_network_id(ready.chain_id.into())
        .with_uniswap_v3_fees(UNISWAP_V3_FEES.to_vec())
        .build()
        .await
        .map_err(|error| {
            ToolError::upstream(format!("Could not discover any price source: {error}"))
        })?;

    // The discovered graph ends at tokens. This is the edge that reaches a
    // currency: one stablecoin, pegged one to one, named in the route so the
    // assumption is visible rather than buried.
    if let Some(peg) = ready.chain.and_then(|chain| chain.usd_anchor()) {
        router.add_quoter(
            FixedQuoter {
                token_in: AssetIdentifier::ERC20 {
                    address: peg.address,
                },
                token_in_decimals: peg.decimals,
                token_out: AssetIdentifier::Fiat {
                    symbol: "usd".to_string(),
                },
                token_out_decimals: peg.decimals,
                fixed_rate: U256::from(1u64),
                fixed_rate_decimals: 0,
            }
            .into(),
        );
    }

    Ok(router)
}
