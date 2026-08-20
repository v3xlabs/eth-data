//! What a chain ID unlocks.
//!
//! These tables are data in the binary, not configuration. A user supplies an
//! RPC URL; everything here follows from the chain ID that endpoint reports.
//! Adding a chain is three lines here, not three URLs in every user's config.

// Which fields of these tables get read depends on which feature groups are
// compiled, so a slim build legitimately leaves some of them unread.
#![allow(dead_code)]

use alloy::primitives::{Address, address};

/// Deployed at the same address on every chain listed below, and on most others.
/// <https://github.com/mds1/multicall>
pub const MULTICALL3: Address = address!("0xcA11bde05977b3631167028862bE2a173976CA11");

/// A token the price router can route through. Anything with deep liquidity
/// against most of the chain's other assets.
#[derive(Debug, Clone, Copy)]
pub struct Anchor {
    pub address: Address,
    pub symbol: &'static str,
    pub decimals: u8,
    /// True when one unit is worth one US dollar closely enough to peg it.
    /// This is what gives the router a `fiat:usd` endpoint to route towards.
    pub usd_peg: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ChainInfo {
    pub chain_id: u64,
    pub label: &'static str,
    /// Public Blockscout v2 API base, no trailing slash. `None` means the
    /// explorer tools report `unsupported_on_network` for this chain.
    pub explorer: Option<&'static str>,
    pub native_symbol: &'static str,
    pub anchors: &'static [Anchor],
}

impl ChainInfo {
    pub fn usd_anchor(&self) -> Option<&Anchor> {
        self.anchors.iter().find(|anchor| anchor.usd_peg)
    }
}

const MAINNET_ANCHORS: &[Anchor] = &[
    Anchor {
        address: address!("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
        symbol: "WETH",
        decimals: 18,
        usd_peg: false,
    },
    Anchor {
        address: address!("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        symbol: "USDC",
        decimals: 6,
        usd_peg: true,
    },
    Anchor {
        address: address!("0xdAC17F958D2ee523a2206206994597C13D831ec7"),
        symbol: "USDT",
        decimals: 6,
        usd_peg: false,
    },
    Anchor {
        address: address!("0x6B175474E89094C44Da98b954EedeAC495271d0F"),
        symbol: "DAI",
        decimals: 18,
        usd_peg: false,
    },
];

const BASE_ANCHORS: &[Anchor] = &[
    Anchor {
        address: address!("0x4200000000000000000000000000000000000006"),
        symbol: "WETH",
        decimals: 18,
        usd_peg: false,
    },
    Anchor {
        address: address!("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
        symbol: "USDC",
        decimals: 6,
        usd_peg: true,
    },
];

const OPTIMISM_ANCHORS: &[Anchor] = &[
    Anchor {
        address: address!("0x4200000000000000000000000000000000000006"),
        symbol: "WETH",
        decimals: 18,
        usd_peg: false,
    },
    Anchor {
        address: address!("0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85"),
        symbol: "USDC",
        decimals: 6,
        usd_peg: true,
    },
];

const ARBITRUM_ANCHORS: &[Anchor] = &[
    Anchor {
        address: address!("0x82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
        symbol: "WETH",
        decimals: 18,
        usd_peg: false,
    },
    Anchor {
        address: address!("0xaf88d065e77c8cC2239327C5EDb3A432268e5831"),
        symbol: "USDC",
        decimals: 6,
        usd_peg: true,
    },
];

const GNOSIS_ANCHORS: &[Anchor] = &[
    Anchor {
        address: address!("0xe91D153E0b41518A2Ce8Dd3D7944Fa863463a97d"),
        symbol: "WXDAI",
        decimals: 18,
        usd_peg: true,
    },
    Anchor {
        address: address!("0xDDAfbb505ad214D7b80b1f830fcCc89B60fb7A83"),
        symbol: "USDC",
        decimals: 6,
        usd_peg: false,
    },
];

const POLYGON_ANCHORS: &[Anchor] = &[
    Anchor {
        address: address!("0x0d500B1d8E8eF31E21C99d1Db9A6444d3ADf1270"),
        symbol: "WPOL",
        decimals: 18,
        usd_peg: false,
    },
    Anchor {
        address: address!("0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359"),
        symbol: "USDC",
        decimals: 6,
        usd_peg: true,
    },
];

/// Testnets carry no anchors on purpose. A price routed through a testnet pool
/// is a number with no meaning, and returning one would be worse than refusing.
const NO_ANCHORS: &[Anchor] = &[];

const CHAINS: &[ChainInfo] = &[
    ChainInfo {
        chain_id: 1,
        label: "Ethereum Mainnet",
        explorer: Some("https://eth.blockscout.com/api/v2"),
        native_symbol: "ETH",
        anchors: MAINNET_ANCHORS,
    },
    ChainInfo {
        chain_id: 11155111,
        label: "Sepolia",
        explorer: Some("https://eth-sepolia.blockscout.com/api/v2"),
        native_symbol: "ETH",
        anchors: NO_ANCHORS,
    },
    ChainInfo {
        chain_id: 560048,
        label: "Hoodi",
        explorer: Some("https://eth-hoodi.blockscout.com/api/v2"),
        native_symbol: "ETH",
        anchors: NO_ANCHORS,
    },
    ChainInfo {
        chain_id: 8453,
        label: "Base",
        explorer: Some("https://base.blockscout.com/api/v2"),
        native_symbol: "ETH",
        anchors: BASE_ANCHORS,
    },
    ChainInfo {
        chain_id: 10,
        label: "OP Mainnet",
        explorer: Some("https://optimism.blockscout.com/api/v2"),
        native_symbol: "ETH",
        anchors: OPTIMISM_ANCHORS,
    },
    ChainInfo {
        chain_id: 42161,
        label: "Arbitrum One",
        explorer: Some("https://arbitrum.blockscout.com/api/v2"),
        native_symbol: "ETH",
        anchors: ARBITRUM_ANCHORS,
    },
    ChainInfo {
        chain_id: 100,
        label: "Gnosis",
        explorer: Some("https://gnosis.blockscout.com/api/v2"),
        native_symbol: "XDAI",
        anchors: GNOSIS_ANCHORS,
    },
    ChainInfo {
        chain_id: 137,
        label: "Polygon",
        explorer: Some("https://polygon.blockscout.com/api/v2"),
        native_symbol: "POL",
        anchors: POLYGON_ANCHORS,
    },
];

/// An unrecognised chain still works for every RPC and fork tool. Only the
/// groups that need a table report themselves unavailable.
pub fn lookup(chain_id: u64) -> Option<&'static ChainInfo> {
    CHAINS.iter().find(|chain| chain.chain_id == chain_id)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn chain_ids_are_unique() {
        let unique: BTreeSet<u64> = CHAINS.iter().map(|chain| chain.chain_id).collect();
        assert_eq!(unique.len(), CHAINS.len());
    }

    #[test]
    fn explorer_bases_have_no_trailing_slash() {
        for chain in CHAINS {
            if let Some(explorer) = chain.explorer {
                assert!(
                    !explorer.ends_with('/'),
                    "{} has a trailing slash",
                    explorer
                );
                assert!(explorer.starts_with("https://"), "{explorer} is not HTTPS");
            }
        }
    }

    #[test]
    fn every_anchored_chain_has_exactly_one_usd_peg() {
        for chain in CHAINS.iter().filter(|chain| !chain.anchors.is_empty()) {
            let pegs = chain.anchors.iter().filter(|a| a.usd_peg).count();
            assert_eq!(pegs, 1, "{} has {pegs} usd pegs", chain.label);
        }
    }

    #[test]
    fn the_three_default_networks_are_known() {
        for chain_id in [1, 11155111, 560048] {
            assert!(lookup(chain_id).is_some(), "chain {chain_id} is missing");
        }
    }
}
