//! Shapes every tool result shares: amounts, block context, source attribution.

use alloy::primitives::U256;
use schemars::JsonSchema;
use serde::Serialize;

/// A base-unit integer with the decimals needed to read it.
///
/// `raw` is a decimal string because a `u256` does not survive JSON's number
/// type, and `formatted` is computed with integer arithmetic so it is exact
/// rather than whatever `f64` rounded it to.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Amount {
    pub raw: String,
    pub decimals: u8,
    pub formatted: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

impl Amount {
    pub fn new(raw: U256, decimals: u8, symbol: Option<String>) -> Self {
        Self {
            formatted: format_units(raw, decimals),
            raw: raw.to_string(),
            decimals,
            symbol,
        }
    }

    pub fn native(raw: U256, symbol: &str) -> Self {
        Self::new(raw, 18, Some(symbol.to_string()))
    }
}

/// Exact fixed-point rendering. No floating point anywhere on this path.
pub fn format_units(value: U256, decimals: u8) -> String {
    if decimals == 0 {
        return value.to_string();
    }
    let digits = value.to_string();
    let decimals = usize::from(decimals);
    let (whole, fraction) = if digits.len() > decimals {
        let split = digits.len() - decimals;
        (digits[..split].to_string(), digits[split..].to_string())
    } else {
        (
            "0".to_string(),
            format!("{:0>width$}", digits, width = decimals),
        )
    };
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        whole
    } else {
        format!("{whole}.{fraction}")
    }
}

/// The block a result was read at. Present wherever a number could change
/// between two calls, so two fields of one answer are known to agree.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BlockContext {
    pub number: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

impl BlockContext {
    pub fn number(number: u64) -> Self {
        Self { number, hash: None }
    }
}

/// Where a field came from, named rather than implied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(not(feature = "explorer"), allow(dead_code))]
pub enum Source {
    /// A direct contract read or RPC call against the configured endpoint.
    OnChain,
    /// A Blockscout index.
    Explorer,
    /// Both agreed.
    ExplorerVerifiedOnChain,
    /// The index said so and the chain was not asked, or disagreed.
    ExplorerUnverified,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_whole_and_fractional_parts() {
        assert_eq!(format_units(U256::from(1_500_000u64), 6), "1.5");
        assert_eq!(format_units(U256::from(1_000_000u64), 6), "1");
        assert_eq!(format_units(U256::from(1u64), 6), "0.000001");
        assert_eq!(format_units(U256::from(0u64), 18), "0");
        assert_eq!(format_units(U256::from(123u64), 0), "123");
    }

    #[test]
    fn one_ether_is_exact() {
        let wei = U256::from(10u64).pow(U256::from(18u64));
        assert_eq!(format_units(wei, 18), "1");
        assert_eq!(
            format_units(wei + U256::from(1u64), 18),
            "1.000000000000000001"
        );
    }

    #[test]
    fn a_large_balance_keeps_every_digit() {
        // f64 would round this. Integer arithmetic does not.
        let raw = U256::from_str_radix("123456789012345678901234567890", 10).unwrap();
        let amount = Amount::new(raw, 18, Some("WEIRD".into()));
        assert_eq!(amount.raw, "123456789012345678901234567890");
        assert_eq!(amount.formatted, "123456789012.34567890123456789");
    }

    #[test]
    fn a_value_smaller_than_one_unit_pads_correctly() {
        assert_eq!(format_units(U256::from(42u64), 18), "0.000000000000000042");
    }
}
