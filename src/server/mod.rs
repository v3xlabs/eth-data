//! The MCP surface: shared state, feature-gated router assembly, server info.

use std::sync::Arc;

use rmcp::{
    ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool_handler,
};

use crate::config::Networks;

pub mod error;
pub mod output;

/// Everything the tools share. One instance for the process lifetime.
pub struct AppState {
    pub networks: Networks,
    #[cfg(feature = "fork")]
    pub forks: crate::fork::Registry,
    #[cfg(feature = "icons")]
    pub icons: eth_icons::IconClient,
    #[cfg(feature = "prices")]
    pub routers: crate::prices::RouterCache,
}

#[derive(Clone)]
pub struct EthData {
    pub state: Arc<AppState>,
    tool_router: ToolRouter<Self>,
}

impl EthData {
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            tool_router: Self::router(),
        }
    }

    /// One router per compiled group. A feature that is off contributes no
    /// tool schema, so a slim build costs the caller nothing to ignore.
    fn router() -> ToolRouter<Self> {
        #[allow(unused_mut)]
        let mut router = Self::chain_router();
        #[cfg(feature = "traces")]
        {
            router += Self::trace_router();
        }
        #[cfg(feature = "ens")]
        {
            router += Self::ens_router();
        }
        #[cfg(feature = "explorer")]
        {
            router += Self::explorer_router();
        }
        #[cfg(feature = "signatures")]
        {
            router += Self::signature_router();
        }
        #[cfg(feature = "icons")]
        {
            router += Self::icon_router();
        }
        #[cfg(feature = "avatars")]
        {
            router += Self::avatar_router();
        }
        #[cfg(feature = "prices")]
        {
            router += Self::price_router();
        }
        #[cfg(feature = "fork")]
        {
            router += Self::fork_router();
        }
        router
    }

    /// The compiled tool set, without needing a resolved network. This is what
    /// `--print-tools` prints and what the schema snapshot test locks.
    pub fn tool_catalog() -> Vec<rmcp::model::Tool> {
        Self::router().list_all()
    }

    pub fn tool_count(&self) -> usize {
        self.tool_router.list_all().len()
    }
}

const INSTRUCTIONS: &str = "\
Read-only Ethereum data over configured JSON-RPC endpoints, plus local Anvil forks.

Networks are named, never URLs. Call network_list first to see which names exist, \
which chain each resolved to, and which capabilities that chain unlocked. A tool that \
returns unsupported_on_network is telling you the network lacks an explorer, a trace \
namespace, or a price anchor, not that the arguments were wrong.

Nothing here writes state. fork_create starts a local Anvil fork and returns its RPC \
URL, port, and funded development accounts; drive that URL yourself with cast, forge, \
or any script in the repository. There is deliberately no tool for setting a balance or \
sending a transaction, because `cast rpc anvil_setBalance --rpc-url <url>` and \
`cast send --unlocked --from <addr> --rpc-url <url>` already do it with better \
arguments and better errors. Call fork_close when you are done, and fork_logs to see \
what your own commands did.";

#[tool_handler(router = self.tool_router)]
impl ServerHandler for EthData {
    fn get_info(&self) -> ServerInfo {
        let implementation = Implementation::new("eth-data", env!("CARGO_PKG_VERSION"))
            .with_title("eth-data")
            .with_description(env!("CARGO_PKG_DESCRIPTION"));

        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(implementation)
            .with_instructions(INSTRUCTIONS)
    }
}
