//! ENS forward, reverse, and text-record resolution, read on chain.
//!
//! This replaces the `cast resolve-name` subprocess the Python server used. No
//! indexer, no subgraph, no API key.

use alloy::primitives::Address;
use rmcp::handler::server::router::tool::ToolRouter;

use crate::{
    config::network::Ready,
    server::{EthData, error::ToolError},
};

pub mod records;
pub mod resolve;

impl EthData {
    pub(crate) fn ens_router() -> ToolRouter<Self> {
        Self::ens_resolve_router() + Self::ens_records_router()
    }

    /// The name path behind `address_of`, so every tool that takes an address
    /// takes a name too.
    pub(crate) async fn resolve_ens_address(
        &self,
        network: &str,
        name: &str,
    ) -> Result<Address, ToolError> {
        let ready = self.state.networks.ready(network).await?;
        require_ens(&ready, network)?;
        Ok(resolve::forward(&ready, &normalize(name)).await?.address)
    }
}

/// The ENS registry, the universal resolver, and the reverse registrar all live
/// at addresses that only exist on mainnet. Attempting a lookup elsewhere would
/// call an empty address and report a confusing decode failure, so it is
/// refused with the actual reason instead.
pub(crate) fn require_ens(ready: &Ready, network: &str) -> Result<(), ToolError> {
    const MAINNET: u64 = 1;
    if ready.chain_id == MAINNET {
        return Ok(());
    }
    Err(ToolError::unsupported(format!(
        "ENS is read from the mainnet registry, and {network:?} is chain {}.",
        ready.chain_id
    ))
    .with_remedy("Use a network whose endpoint serves Ethereum mainnet.".to_string()))
}

/// A normalized name, and whether normalization was complete.
///
/// Full ENSIP-15 normalization is not implemented. ASCII case folding is, which
/// covers the names people actually type. A name with non-ASCII characters is
/// passed through as written and flagged, because silently hashing an
/// unnormalized label would resolve to the wrong node.
pub(crate) struct Normalized {
    pub name: String,
    pub fully_normalized: bool,
}

pub(crate) fn normalize(name: &str) -> Normalized {
    let trimmed = name.trim();
    if trimmed.is_ascii() {
        return Normalized {
            name: trimmed.to_ascii_lowercase(),
            fully_normalized: true,
        };
    }
    Normalized {
        name: trimmed.to_string(),
        fully_normalized: false,
    }
}

pub(crate) fn check_name(name: &Normalized) -> Result<(), ToolError> {
    const MAX_NAME_BYTES: usize = 255;
    if name.name.is_empty() || name.name.len() > MAX_NAME_BYTES {
        return Err(ToolError::invalid_input(format!(
            "A name must be between 1 and {MAX_NAME_BYTES} bytes."
        )));
    }
    if !name.name.contains('.') {
        return Err(ToolError::invalid_input(format!(
            "{:?} has no top-level label. Names look like `vitalik.eth`.",
            name.name
        )));
    }
    if name.name.split('.').any(str::is_empty) {
        return Err(ToolError::invalid_input(format!(
            "{:?} has an empty label.",
            name.name
        )));
    }
    Ok(())
}

pub(crate) fn ens_error(action: &str, error: impl std::fmt::Display) -> ToolError {
    ToolError::upstream(format!("{action} failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_names_are_case_folded() {
        let normalized = normalize("  VITALIK.ETH ");
        assert_eq!(normalized.name, "vitalik.eth");
        assert!(normalized.fully_normalized);
    }

    #[test]
    fn non_ascii_names_are_flagged_rather_than_mangled() {
        let normalized = normalize("🚀.eth");
        assert_eq!(normalized.name, "🚀.eth");
        assert!(!normalized.fully_normalized);
    }

    #[test]
    fn rejects_a_bare_label() {
        assert!(check_name(&normalize("vitalik")).is_err());
    }

    #[test]
    fn rejects_an_empty_label() {
        assert!(check_name(&normalize("a..eth")).is_err());
        assert!(check_name(&normalize(".eth")).is_err());
    }

    #[test]
    fn accepts_a_subdomain() {
        check_name(&normalize("pay.vitalik.eth")).unwrap();
    }
}
