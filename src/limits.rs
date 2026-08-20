//! Every bound the server enforces, and the parsers that enforce them.
//!
//! Nothing in this module makes a network call. A value that fails here never
//! reaches an upstream, which is the property the property tests assert.

// This is the catalogue of every bound the server has. Which ones are read
// depends on which feature groups are compiled.
#![allow(dead_code)]

use std::time::Duration;

use alloy::primitives::{Address, B256, Bytes, U256};

use crate::server::error::ToolError;

pub const MAX_RESPONSE_BYTES: usize = 1_048_576;
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const MAX_REDIRECTS: usize = 3;

pub const MAX_LOG_BLOCK_RANGE: u64 = 10_000;
pub const MAX_LOG_TOPICS: usize = 4;

pub const DEFAULT_PAGE_SIZE: u32 = 50;
pub const MAX_PAGE_SIZE: u32 = 100;
pub const MAX_CURSOR_BYTES: usize = 1_000;

pub const MAX_SIGNATURE_BYTES: usize = 1_000;
pub const MAX_ABI_ARGUMENTS: usize = 64;

pub const MAX_MULTICALL_BATCH: usize = 100;

pub const MAX_LIVE_FORKS: usize = 8;
pub const FORK_READY_TIMEOUT: Duration = Duration::from_secs(30);
pub const FORK_STOP_TIMEOUT: Duration = Duration::from_secs(5);
pub const FORK_LOG_LINES: usize = 500;
pub const MAX_FORK_LOG_REQUEST: usize = 200;

/// A block number, a tag, or a block hash, as accepted by the RPC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockSelector {
    Tag(BlockTag),
    Number(u64),
    Hash(B256),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockTag {
    Latest,
    Earliest,
    Pending,
    Safe,
    Finalized,
}

impl BlockTag {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "latest" => Some(Self::Latest),
            "earliest" => Some(Self::Earliest),
            "pending" => Some(Self::Pending),
            "safe" => Some(Self::Safe),
            "finalized" => Some(Self::Finalized),
            _ => None,
        }
    }
}

impl BlockSelector {
    pub fn parse(value: &str) -> Result<Self, ToolError> {
        if let Some(tag) = BlockTag::parse(value) {
            return Ok(Self::Tag(tag));
        }
        if let Some(hex) = value.strip_prefix("0x")
            && hex.len() == 64
        {
            return parse_hash(value, "block").map(Self::Hash);
        }
        parse_u64(value, "block").map(Self::Number)
    }
}

impl Default for BlockSelector {
    fn default() -> Self {
        Self::Tag(BlockTag::Latest)
    }
}

pub fn parse_address(value: &str) -> Result<Address, ToolError> {
    value.parse::<Address>().map_err(|_| {
        ToolError::invalid_input(format!(
            "Expected a 20-byte 0x-prefixed address, received {}.",
            elide(value)
        ))
    })
}

pub fn parse_hash(value: &str, label: &str) -> Result<B256, ToolError> {
    value.parse::<B256>().map_err(|_| {
        ToolError::invalid_input(format!(
            "Expected {label} to be a 32-byte 0x-prefixed hash, received {}.",
            elide(value)
        ))
    })
}

pub fn parse_bytes(value: &str, label: &str) -> Result<Bytes, ToolError> {
    value.parse::<Bytes>().map_err(|_| {
        ToolError::invalid_input(format!(
            "Expected {label} to be even-length 0x-prefixed hex, received {}.",
            elide(value)
        ))
    })
}

/// Decimal or 0x-prefixed hexadecimal, as a 256-bit unsigned quantity.
pub fn parse_u256(value: &str, label: &str) -> Result<U256, ToolError> {
    let parsed = match value.strip_prefix("0x") {
        Some(hex) => U256::from_str_radix(hex, 16),
        None => U256::from_str_radix(value, 10),
    };
    parsed.map_err(|_| {
        ToolError::invalid_input(format!(
            "Expected {label} to be a decimal or 0x-prefixed quantity, received {}.",
            elide(value)
        ))
    })
}

pub fn parse_u64(value: &str, label: &str) -> Result<u64, ToolError> {
    let parsed = match value.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => value.parse::<u64>().ok(),
    };
    parsed.ok_or_else(|| {
        ToolError::invalid_input(format!(
            "Expected {label} to be a decimal or 0x-prefixed quantity, received {}.",
            elide(value)
        ))
    })
}

pub fn check_log_range(from_block: u64, to_block: u64) -> Result<(), ToolError> {
    if to_block < from_block {
        return Err(ToolError::invalid_input(
            "to_block must not be below from_block.",
        ));
    }
    if to_block - from_block > MAX_LOG_BLOCK_RANGE {
        return Err(ToolError::invalid_input(format!(
            "Log ranges are limited to {MAX_LOG_BLOCK_RANGE} blocks, requested {}.",
            to_block - from_block
        )));
    }
    Ok(())
}

pub fn check_page_size(page_size: Option<u32>) -> Result<u32, ToolError> {
    let size = page_size.unwrap_or(DEFAULT_PAGE_SIZE);
    if size == 0 || size > MAX_PAGE_SIZE {
        return Err(ToolError::invalid_input(format!(
            "page_size must be between 1 and {MAX_PAGE_SIZE}, received {size}."
        )));
    }
    Ok(size)
}

pub fn check_cursor(cursor: Option<&str>) -> Result<Option<String>, ToolError> {
    match cursor {
        None => Ok(None),
        Some(value) if value.len() > MAX_CURSOR_BYTES => Err(ToolError::invalid_input(format!(
            "cursor exceeds {MAX_CURSOR_BYTES} characters."
        ))),
        Some(value) => Ok(Some(value.to_string())),
    }
}

pub fn check_signature(signature: &str) -> Result<(), ToolError> {
    if signature.is_empty() || signature.len() > MAX_SIGNATURE_BYTES {
        return Err(ToolError::invalid_input(format!(
            "Signature must be between 1 and {MAX_SIGNATURE_BYTES} characters."
        )));
    }
    Ok(())
}

/// Keeps a rejected value out of the error message when it is long enough to be
/// a paste of something else entirely.
fn elide(value: &str) -> String {
    const LIMIT: usize = 80;
    if value.len() <= LIMIT {
        return format!("{value:?}");
    }
    let head: String = value.chars().take(LIMIT).collect();
    format!("{head:?} (truncated, {} characters)", value.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short_address() {
        assert!(parse_address("0x1234").is_err());
    }

    #[test]
    fn accepts_mixed_case_address() {
        assert!(parse_address("0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045").is_ok());
    }

    #[test]
    fn quantity_accepts_decimal_and_hex() {
        assert_eq!(parse_u256("255", "value").unwrap(), U256::from(255));
        assert_eq!(parse_u256("0xff", "value").unwrap(), U256::from(255));
    }

    #[test]
    fn quantity_rejects_negative() {
        assert!(parse_u256("-1", "value").is_err());
    }

    #[test]
    fn block_selector_distinguishes_hash_from_number() {
        let hash = format!("0x{}", "11".repeat(32));
        assert!(matches!(
            BlockSelector::parse(&hash).unwrap(),
            BlockSelector::Hash(_)
        ));
        assert!(matches!(
            BlockSelector::parse("0x1234").unwrap(),
            BlockSelector::Number(0x1234)
        ));
        assert!(matches!(
            BlockSelector::parse("latest").unwrap(),
            BlockSelector::Tag(BlockTag::Latest)
        ));
    }

    #[test]
    fn log_range_is_bounded() {
        assert!(check_log_range(0, MAX_LOG_BLOCK_RANGE).is_ok());
        assert!(check_log_range(0, MAX_LOG_BLOCK_RANGE + 1).is_err());
        assert!(check_log_range(10, 9).is_err());
    }

    #[test]
    fn long_rejected_values_are_elided() {
        let long = "0x".to_string() + &"a".repeat(500);
        let message = parse_address(&long).unwrap_err().message;
        assert!(message.len() < 200, "message was {message}");
        assert!(message.contains("truncated"));
    }
}
