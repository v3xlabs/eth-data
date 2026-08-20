//! `price_get` and `price_route`, priced from the chain the server is already
//! connected to.
//!
//! No exchange API, no aggregator, no key. Every result carries the route it
//! took and the block it read, so a number here is checkable rather than
//! asserted.

use alloy::providers::Provider;
use eth_prices::{
    asset::{Asset, AssetIdentifier},
    network::NetworkInstant,
    router::Router,
};
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    config::network::Ready,
    prices::router::{USD, anchors_of},
    server::{
        EthData,
        error::ToolError,
        output::{Amount, BlockContext},
    },
};

pub mod router;

pub use router::RouterCache;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PriceParams {
    /// A name from `network_list` whose `capabilities.prices` is true.
    pub network: String,
    /// An ERC-20 address, `native` for the chain's own currency, or
    /// `fiat:<symbol>`.
    pub asset: String,
    /// What to price it in. An ERC-20 address, `native`, or `fiat:usd`.
    /// Defaults to `fiat:usd`.
    pub quote: Option<String>,
    /// Price at this block instead of the head, for a reproducible number.
    pub block: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RouteParams {
    /// A name from `network_list` whose `capabilities.prices` is true.
    pub network: String,
    /// An ERC-20 address, `native`, or `fiat:<symbol>`.
    pub asset: String,
    /// What to route towards. Defaults to `fiat:usd`.
    pub quote: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PriceResult {
    pub network: String,
    pub asset: AssetSummary,
    pub quote: AssetSummary,
    /// What one whole unit of `asset` is worth in `quote`.
    pub price: Amount,
    /// The block every hop was read at.
    pub read_at: BlockContext,
    /// The hops the price went through, in order. A long route compounds the
    /// error of every pool in it.
    pub route: Vec<Hop>,
    pub source: &'static str,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RouteResult {
    pub network: String,
    pub asset: String,
    pub quote: String,
    /// The quoters discovered for this chain, whether or not the route uses
    /// them.
    pub discovered_quoters: usize,
    pub route: Vec<Hop>,
    pub source: &'static str,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Hop {
    /// The quoter's own identity, such as `uniswap_v3:0x8ad5...` or
    /// `fixed:0xa0b8...:fiat:usd`.
    pub quoter: String,
    pub from: String,
    pub to: String,
    /// Higher means this edge was preferred during routing.
    pub confidence: u64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AssetSummary {
    pub identifier: String,
    pub symbol: String,
    pub decimals: u8,
}

fn parse_asset(value: &str, label: &str) -> Result<AssetIdentifier, ToolError> {
    AssetIdentifier::try_from(value).map_err(|error| {
        ToolError::invalid_input(format!("{label} {value:?} is not an asset: {error}"))
    })
}

fn hops(route: &eth_prices::router::route::Route) -> Vec<Hop> {
    route
        .path
        .iter()
        .map(|step| {
            let (token_in, token_out) = step.quoter.tokens();
            let forward = matches!(step.direction, eth_prices::quoter::RateDirection::Forward);
            let (from, to) = if forward {
                (token_in, token_out)
            } else {
                (token_out, token_in)
            };
            Hop {
                quoter: step.quoter.identity(),
                from: from.to_string(),
                to: to.to_string(),
                confidence: step.quoter.confidence,
            }
        })
        .collect()
}

async fn prepare(
    server: &EthData,
    network: &str,
    asset: &str,
    quote: Option<&str>,
) -> Result<
    (
        Ready,
        AssetIdentifier,
        AssetIdentifier,
        std::sync::Arc<Router>,
    ),
    ToolError,
> {
    let ready = server.state.networks.ready(network).await?;
    anchors_of(&ready, network)?;

    let asset = parse_asset(asset, "asset")?;
    let quote = parse_asset(quote.unwrap_or(USD), "quote")?;
    if asset == quote {
        return Err(ToolError::invalid_input(
            "asset and quote are the same, so the price is one.".to_string(),
        ));
    }

    let router = server.state.routers.get(&ready, &asset, &quote).await?;
    Ok((ready, asset, quote, router))
}

#[tool_router(router = price_router, vis = "pub(crate)")]
impl EthData {
    /// Price one asset in another, read from on-chain pools and feeds.
    ///
    /// Routes through Uniswap v2 and v3, Chainlink, and ERC-4626 vaults, ending
    /// at a stablecoin pegged to `fiat:usd`. The route and the block are part of
    /// the answer: check them before trusting the number.
    #[tool(name = "price_get")]
    pub async fn price_get(
        &self,
        Parameters(params): Parameters<PriceParams>,
    ) -> Result<Json<PriceResult>, ToolError> {
        let (ready, asset, quote, router) = prepare(
            self,
            &params.network,
            &params.asset,
            params.quote.as_deref(),
        )
        .await?;

        let route = router.compute(&asset, &quote).map_err(|error| {
            ToolError::unsupported(format!(
                "No route from {asset} to {quote} on chain {}: {error}",
                ready.chain_id
            ))
            .with_remedy(format!(
                "price_route shows which quoters were discovered. A token with no pool \
                 against one of this chain's anchors ({}) cannot be priced.",
                ready
                    .chain
                    .map(|chain| chain.anchors)
                    .unwrap_or(&[])
                    .iter()
                    .map(|anchor| anchor.symbol)
                    .collect::<Vec<_>>()
                    .join(", "),
            ))
        })?;

        let block =
            match params.block {
                Some(block) => block,
                None => ready.provider.get_block_number().await.map_err(|error| {
                    ToolError::upstream(format!("Could not pin a block: {error}"))
                })?,
            };

        let (asset_meta, quote_meta) = futures::join!(
            Asset::new(asset.clone(), &ready.provider),
            Asset::new(quote.clone(), &ready.provider),
        );
        let asset_meta = asset_meta
            .map_err(|error| ToolError::upstream(format!("Could not read {asset}: {error}")))?;
        let quote_meta = quote_meta
            .map_err(|error| ToolError::upstream(format!("Could not read {quote}: {error}")))?;

        let networks = NetworkInstant::default().with_evm_block(
            ready.chain_id.into(),
            block,
            ready.provider.clone(),
        );

        let amount_out = route
            .quote(&networks, asset_meta.nominal_amount())
            .await
            .map_err(|error| ToolError::upstream(format!("Quoting failed: {error}")))?;

        Ok(Json(PriceResult {
            network: params.network,
            price: Amount::new(
                amount_out,
                quote_meta.decimals,
                Some(quote_meta.symbol.clone()),
            ),
            asset: AssetSummary {
                identifier: asset.to_string(),
                symbol: asset_meta.symbol,
                decimals: asset_meta.decimals,
            },
            quote: AssetSummary {
                identifier: quote.to_string(),
                symbol: quote_meta.symbol,
                decimals: quote_meta.decimals,
            },
            read_at: BlockContext::number(block),
            route: hops(&route),
            source: "eth-prices",
        }))
    }

    /// Show the route a price would take, without quoting it.
    ///
    /// Use this when `price_get` says there is no route, or when a price looks
    /// wrong and you want to see which pools produced it.
    #[tool(name = "price_route")]
    pub async fn price_route(
        &self,
        Parameters(params): Parameters<RouteParams>,
    ) -> Result<Json<RouteResult>, ToolError> {
        let (ready, asset, quote, router) = prepare(
            self,
            &params.network,
            &params.asset,
            params.quote.as_deref(),
        )
        .await?;

        let route = router.compute(&asset, &quote).map_err(|error| {
            ToolError::unsupported(format!(
                "No route from {asset} to {quote} on chain {}: {error}. \
                 {} quoters were discovered.",
                ready.chain_id,
                router.quoters.len()
            ))
        })?;

        Ok(Json(RouteResult {
            network: params.network,
            asset: asset.to_string(),
            quote: quote.to_string(),
            discovered_quoters: router.quoters.len(),
            route: hops(&route),
            source: "eth-prices",
        }))
    }
}
