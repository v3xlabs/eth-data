//! Read-only JSON-RPC against a configured network, and the local ABI codec.
//!
//! This group is always compiled. Everything else in the server is a feature.

use alloy::{
    eips::{BlockId, BlockNumberOrTag},
    primitives::Address,
};
use rmcp::handler::server::router::tool::ToolRouter;

use crate::{
    config::network::Ready,
    limits::{BlockSelector, BlockTag, parse_address},
    server::{EthData, error::ToolError},
};

pub mod abi;
pub mod account;
pub mod block;
pub mod call;
pub mod logs;
pub mod meta;
#[cfg(feature = "traces")]
pub mod trace;

impl EthData {
    pub(crate) fn chain_router() -> ToolRouter<Self> {
        Self::meta_router()
            + Self::block_router()
            + Self::account_router()
            + Self::call_router()
            + Self::logs_router()
            + Self::abi_router()
    }

    /// An address, or a name the ENS group can resolve. Without the `ens`
    /// feature the name path is not a silent failure: it says the feature is
    /// missing rather than pretending the name was malformed.
    pub(crate) async fn address_of(
        &self,
        network: &str,
        value: &str,
    ) -> Result<Address, ToolError> {
        if let Ok(address) = parse_address(value) {
            return Ok(address);
        }

        #[cfg(feature = "ens")]
        {
            if value.contains('.') {
                return self.resolve_ens_address(network, value).await;
            }
            Err(ToolError::invalid_input(format!(
                "Expected an address or an ENS name, received {value:?}."
            )))
        }

        #[cfg(not(feature = "ens"))]
        {
            let _ = network;
            if value.contains('.') {
                return Err(ToolError::unsupported(format!(
                    "{value:?} looks like a name, but this build has no ENS support."
                ))
                .with_remedy("Rebuild with the `ens` feature, or pass an address.".to_string()));
            }
            Err(ToolError::invalid_input(format!(
                "Expected a 20-byte 0x-prefixed address, received {value:?}."
            )))
        }
    }
}

/// Parses the optional `block` argument every read accepts.
pub(crate) fn block_id(block: Option<&str>) -> Result<BlockId, ToolError> {
    let selector = match block {
        None => BlockSelector::default(),
        Some(value) => BlockSelector::parse(value)?,
    };
    Ok(match selector {
        BlockSelector::Hash(hash) => BlockId::Hash(hash.into()),
        BlockSelector::Number(number) => BlockId::Number(BlockNumberOrTag::Number(number)),
        BlockSelector::Tag(tag) => BlockId::Number(match tag {
            BlockTag::Latest => BlockNumberOrTag::Latest,
            BlockTag::Earliest => BlockNumberOrTag::Earliest,
            BlockTag::Pending => BlockNumberOrTag::Pending,
            BlockTag::Safe => BlockNumberOrTag::Safe,
            BlockTag::Finalized => BlockNumberOrTag::Finalized,
        }),
    })
}

/// Every RPC failure funnels through here so the caller always gets a category
/// rather than a transport string.
pub(crate) fn rpc_error(method: &str, error: impl std::fmt::Display) -> ToolError {
    let text = error.to_string();
    if text.contains("timed out") || text.contains("timeout") {
        return ToolError::upstream_timeout(format!("{method} timed out: {text}"));
    }
    ToolError::upstream(format!("{method} failed: {text}"))
}

pub(crate) fn native_symbol(ready: &Ready) -> &'static str {
    ready
        .chain
        .map(|chain| chain.native_symbol)
        .unwrap_or("ETH")
}
