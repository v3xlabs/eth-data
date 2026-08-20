//! `address_tokens`: the one tool here that reads from more than one source.
//!
//! The Python server forwarded a Blockscout page and returned whatever the
//! indexer said, with no block and no way to tell a stale index from a real
//! balance. Discovery still has to come from an index, because scanning the
//! chain for "every token this address has ever touched" is unbounded. The
//! numbers do not: once the token list is known, one Multicall3 read settles
//! every balance at one block.

use alloy::{
    primitives::{Address, U256},
    providers::Provider,
    sol,
    sol_types::SolCall,
};
use rmcp::{Json, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    config::derived::MULTICALL3,
    explorer::require_explorer,
    limits::MAX_MULTICALL_BATCH,
    server::{
        EthData,
        error::ToolError,
        output::{Amount, BlockContext, Source},
    },
};

sol! {
    #[sol(rpc)]
    interface IMulticall3 {
        struct Call3 {
            address target;
            bool allowFailure;
            bytes callData;
        }
        struct Result {
            bool success;
            bytes returnData;
        }
        function aggregate3(Call3[] calls) external payable returns (Result[] returnData);
    }
}

sol! {
    #[sol(rpc)]
    interface IERC20 {
        function balanceOf(address owner) external view returns (uint256);
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TokensParams {
    /// A name from `network_list` whose `capabilities.explorer` is true.
    pub network: String,
    /// An address, or an ENS name when the `ens` feature is compiled in.
    pub address: String,
    /// How many tokens to return, 1 to 100. Defaults to 50.
    pub page_size: Option<u32>,
    /// A `next_cursor` from a previous call.
    pub cursor: Option<String>,
    /// Which token standards to include. Defaults to ERC-20 only, because a
    /// wallet with thousands of airdropped NFTs is what `address_nfts` is for.
    pub token_types: Option<Vec<String>>,
    /// Skip the on-chain verification pass and return the indexer's numbers.
    /// Faster, and the balances may be stale. Defaults to false.
    pub skip_verification: Option<bool>,
    /// Include icon candidate URLs. Costs nothing: the URLs are constructed,
    /// not fetched. Defaults to true.
    pub include_icons: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TokensResult {
    pub network: String,
    pub address: String,
    /// The block every verified balance was read at, so two rows of this answer
    /// are known to agree with each other.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<BlockContext>,
    pub tokens: Vec<TokenBalance>,
    /// Pass this back as `cursor` for the next page. Absent means this was the
    /// last one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Names every upstream that contributed to this answer.
    pub sources: Vec<String>,
    /// Set when verification was asked for and could not be done.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_note: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TokenBalance {
    pub token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub token_type: String,
    pub balance: Amount,
    pub source: Source,
    /// Present only when the index and the chain disagreed. The index is
    /// usually behind; occasionally it is wrong. Either way it is worth seeing
    /// rather than having smoothed over.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_balance: Option<String>,
    #[cfg(feature = "icons")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub icons: Vec<crate::assets::icons::IconCandidate>,
}

/// One row as the indexer reported it, before the chain is asked.
struct Indexed {
    address: Option<Address>,
    raw_address: String,
    symbol: Option<String>,
    name: Option<String>,
    decimals: u8,
    token_type: String,
    value: U256,
}

fn field<'a>(row: &'a Value, key: &str) -> Option<&'a str> {
    row.get(key).and_then(Value::as_str)
}

fn parse_row(row: &Value) -> Option<Indexed> {
    let token = row.get("token")?;
    let raw_address = field(token, "address")
        .or_else(|| field(token, "address_hash"))?
        .to_string();

    Some(Indexed {
        address: raw_address.parse::<Address>().ok(),
        raw_address,
        symbol: field(token, "symbol").map(str::to_string),
        name: field(token, "name").map(str::to_string),
        // Blockscout sends decimals as a string, and omits it for NFTs.
        decimals: field(token, "decimals")
            .and_then(|text| text.parse::<u8>().ok())
            .unwrap_or(0),
        token_type: field(token, "type").unwrap_or("unknown").to_string(),
        value: field(row, "value")
            .and_then(|text| U256::from_str_radix(text, 10).ok())
            .unwrap_or(U256::ZERO),
    })
}

#[tool_router(router = balances_router, vis = "pub(crate)")]
impl EthData {
    /// List an address's token balances, discovered through the explorer and
    /// verified on chain.
    ///
    /// The token list comes from the indexer, because the chain cannot be asked
    /// "which tokens has this address held" in bounded time. Every balance is
    /// then re-read through Multicall3 at one pinned block, so the numbers are
    /// authoritative and mutually consistent. A row where the two disagree is
    /// flagged rather than overwritten.
    #[tool(name = "address_tokens")]
    pub async fn address_tokens(
        &self,
        Parameters(params): Parameters<TokensParams>,
    ) -> Result<Json<TokensResult>, ToolError> {
        let ready = self.state.networks.ready(&params.network).await?;
        let base = require_explorer(&ready, &params.network)?;
        let owner = self.address_of(&params.network, &params.address).await?;

        // Stage 1: discovery. `/tokens` is the paginated endpoint;
        // `/token-balances` returns every token an address has ever been sent,
        // which for a well-known wallet is megabytes of airdrop dust.
        let types = params
            .token_types
            .unwrap_or_else(|| vec!["ERC-20".to_string()])
            .join(",");
        let page = crate::explorer::page(
            &ready,
            &params.network,
            &format!("/addresses/{owner}/tokens"),
            params.page_size,
            params.cursor.as_deref(),
            &[("type", types.as_str())],
        )
        .await?;

        let next_cursor = page.next_cursor.clone();
        let rows: Vec<Indexed> = page.items.iter().filter_map(parse_row).collect();

        let mut sources = vec![base.to_string()];

        // Stage 2: verification.
        let verify = !params.skip_verification.unwrap_or(false);
        let mut verified: Vec<Option<U256>> = vec![None; rows.len()];
        let mut verified_at = None;
        let mut verification_note = None;

        if verify && !rows.is_empty() {
            match verify_balances(&ready, owner, &rows).await {
                Ok((block, balances)) => {
                    verified = balances;
                    verified_at = Some(BlockContext::number(block));
                    sources.push(format!("multicall3:{MULTICALL3}"));
                }
                Err(error) => verification_note = Some(error.message),
            }
        } else if !verify {
            verification_note =
                Some("Verification was skipped, so these are the indexer's numbers.".to_string());
        }

        #[cfg(feature = "icons")]
        let want_icons = params.include_icons.unwrap_or(true);
        #[cfg(feature = "icons")]
        if want_icons {
            sources.push("eth-icons".to_string());
        }
        #[cfg(not(feature = "icons"))]
        let _ = params.include_icons;

        let tokens = rows
            .into_iter()
            .zip(verified)
            .map(|(row, on_chain)| {
                let (balance, source, indexed) = match on_chain {
                    Some(value) if value == row.value => {
                        (value, Source::ExplorerVerifiedOnChain, None)
                    }
                    Some(value) => (
                        value,
                        Source::ExplorerVerifiedOnChain,
                        Some(row.value.to_string()),
                    ),
                    None => (row.value, Source::ExplorerUnverified, None),
                };

                TokenBalance {
                    #[cfg(feature = "icons")]
                    icons: match (want_icons, row.address) {
                        (true, Some(address)) => crate::assets::icons::erc20_candidates(
                            &self.state.icons,
                            ready.chain_id,
                            address,
                        ),
                        _ => Vec::new(),
                    },
                    token: row.raw_address,
                    balance: Amount::new(balance, row.decimals, row.symbol.clone()),
                    symbol: row.symbol,
                    name: row.name,
                    token_type: row.token_type,
                    source,
                    indexed_balance: indexed,
                }
            })
            .collect();

        Ok(Json(TokensResult {
            network: params.network,
            address: owner.to_string(),
            verified_at,
            tokens,
            next_cursor,
            sources,
            verification_note,
        }))
    }
}

/// One `eth_call` at one block for every balance on the page.
async fn verify_balances(
    ready: &crate::config::network::Ready,
    owner: Address,
    rows: &[Indexed],
) -> Result<(u64, Vec<Option<U256>>), ToolError> {
    let block = ready
        .provider
        .get_block_number()
        .await
        .map_err(|error| ToolError::upstream(format!("Could not pin a block: {error}")))?;

    // Only fungible balances are a single uint256. An ERC-721 balanceOf returns
    // a token count, which is not the same quantity, so it is left to the index.
    let targets: Vec<(usize, Address)> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.token_type.starts_with("ERC-20"))
        .filter_map(|(index, row)| row.address.map(|address| (index, address)))
        .take(MAX_MULTICALL_BATCH)
        .collect();

    let mut balances = vec![None; rows.len()];
    if targets.is_empty() {
        return Ok((block, balances));
    }

    let calls: Vec<IMulticall3::Call3> = targets
        .iter()
        .map(|(_, token)| IMulticall3::Call3 {
            target: *token,
            allowFailure: true,
            callData: IERC20::balanceOfCall { owner }.abi_encode().into(),
        })
        .collect();

    let multicall = IMulticall3::new(MULTICALL3, &ready.provider);
    let results = multicall
        .aggregate3(calls)
        .block(block.into())
        .call()
        .await
        .map_err(|error| {
            ToolError::upstream(format!(
                "Multicall3 at {MULTICALL3} did not answer on chain {}: {error}",
                ready.chain_id
            ))
        })?;

    for ((index, _), result) in targets.iter().zip(results) {
        if !result.success || result.returnData.len() < 32 {
            continue;
        }
        balances[*index] = Some(U256::from_be_slice(&result.returnData[..32]));
    }

    Ok((block, balances))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_a_blockscout_row() {
        let row = json!({
            "token": {
                "address": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
                "symbol": "USDC",
                "name": "USD Coin",
                "decimals": "6",
                "type": "ERC-20",
            },
            "value": "1500000",
        });
        let parsed = parse_row(&row).unwrap();
        assert_eq!(parsed.decimals, 6);
        assert_eq!(parsed.value, U256::from(1_500_000u64));
        assert_eq!(parsed.symbol.as_deref(), Some("USDC"));
        assert!(parsed.address.is_some());
    }

    #[test]
    fn an_nft_row_without_decimals_reads_as_zero() {
        let row = json!({
            "token": {
                "address": "0xBC4CA0EdA7647A8aB7C2061c2E118A18a936f13D",
                "symbol": "BAYC",
                "type": "ERC-721",
            },
            "value": "3",
        });
        let parsed = parse_row(&row).unwrap();
        assert_eq!(parsed.decimals, 0);
        assert_eq!(parsed.token_type, "ERC-721");
    }

    #[test]
    fn a_row_with_no_token_address_is_dropped() {
        assert!(parse_row(&json!({ "value": "1" })).is_none());
    }

    #[test]
    fn an_unparseable_value_reads_as_zero_rather_than_dropping_the_token() {
        let row = json!({
            "token": { "address": "0x0000000000000000000000000000000000000001", "type": "ERC-20" },
            "value": "not a number",
        });
        assert_eq!(parse_row(&row).unwrap().value, U256::ZERO);
    }
}
