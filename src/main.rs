//! eth-data: a read-only Model Context Protocol server for Ethereum data.
//!
//! Protocol goes to stdout and nothing else does. Logs go to stderr.

use std::{
    path::PathBuf,
    process::ExitCode,
    sync::{Arc, OnceLock},
    time::Duration,
};

use rmcp::{ServiceExt, transport::stdio};
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, fmt};

mod chain;
mod config;
mod limits;
mod server;

#[cfg(any(feature = "icons", feature = "avatars"))]
mod assets;
#[cfg(feature = "ens")]
mod ens;
#[cfg(feature = "explorer")]
mod explorer;
#[cfg(feature = "fork")]
mod fork;
#[cfg(feature = "prices")]
mod prices;
#[cfg(feature = "signatures")]
mod signatures;

use crate::{
    config::{Config, Networks},
    server::{AppState, EthData},
};

/// One client for every outbound request the server makes, carrying the
/// connect, request, and redirect limits in one place.
pub fn http_client() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(limits::CONNECT_TIMEOUT)
                .timeout(limits::REQUEST_TIMEOUT)
                .redirect(reqwest::redirect::Policy::limited(limits::MAX_REDIRECTS))
                .user_agent(concat!("eth-data-mcp/", env!("CARGO_PKG_VERSION")))
                .pool_idle_timeout(Duration::from_secs(60))
                .build()
                .expect("the HTTP client configuration is a constant")
        })
        .clone()
}

struct Args {
    config: Option<PathBuf>,
    print_tools: bool,
}

enum Startup {
    Run(Args),
    Print(String),
    Usage(String),
}

fn parse_args() -> Startup {
    const USAGE: &str = "\
eth-data-mcp: a read-only MCP server for Ethereum data.

Usage: eth-data-mcp [OPTIONS]

Options:
  -c, --config <PATH>  Read this TOML file instead of the default location
      --print-tools    Print the tool catalogue as JSON and exit
  -V, --version        Print the version and exit
  -h, --help           Print this message

Configuration is one field per network, an RPC URL:

  [networks]
  mainnet = \"https://ethereum.reth.rs/rpc\"

Or from the environment: ETH_DATA_NETWORKS__MAINNET=https://...";

    let mut args = Args {
        config: None,
        print_tools: false,
    };
    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "-h" | "--help" => return Startup::Print(USAGE.to_string()),
            "-V" | "--version" => {
                return Startup::Print(format!("eth-data-mcp {}", env!("CARGO_PKG_VERSION")));
            }
            "--print-tools" => args.print_tools = true,
            "-c" | "--config" => match argv.next() {
                Some(path) => args.config = Some(PathBuf::from(path)),
                None => return Startup::Usage("--config needs a path".to_string()),
            },
            other => return Startup::Usage(format!("unrecognised argument {other:?}")),
        }
    }
    Startup::Run(args)
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = match parse_args() {
        Startup::Run(args) => args,
        Startup::Print(text) => {
            println!("{text}");
            return ExitCode::SUCCESS;
        }
        Startup::Usage(reason) => {
            eprintln!("eth-data-mcp: {reason}. Try --help.");
            return ExitCode::FAILURE;
        }
    };

    fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_env("ETH_DATA_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_ansi(false)
        .init();

    if args.print_tools {
        let catalog = EthData::tool_catalog();
        match serde_json::to_string_pretty(&catalog) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                error!(%error, "could not serialise the tool catalogue");
                return ExitCode::FAILURE;
            }
        }
        return ExitCode::SUCCESS;
    }

    match run(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<(), String> {
    let config = Config::load(args.config.as_deref()).map_err(|error| error.to_string())?;
    let networks = Networks::resolve_all(&config).await?;

    let state = Arc::new(AppState {
        networks,
        #[cfg(feature = "fork")]
        forks: fork::Registry::new(),
        #[cfg(feature = "icons")]
        icons: assets::icons::client(),
        #[cfg(feature = "prices")]
        routers: prices::RouterCache::new(),
    });

    let server = EthData::new(Arc::clone(&state));
    info!(tools = server.tool_count(), "serving on stdio");

    let service = server
        .serve(stdio())
        .await
        .map_err(|error| format!("could not start the stdio transport: {error}"))?;

    // A fork outliving the server it belongs to is an orphan process holding a
    // port and a few hundred megabytes of forked state. Reaping is done here
    // rather than in a Drop, and on Linux the children also carry
    // PR_SET_PDEATHSIG so a SIGKILL cannot leave one behind either.
    let outcome = tokio::select! {
        quit = service.waiting() => quit
            .map(|reason| info!(?reason, "client disconnected"))
            .map_err(|error| format!("the service task failed: {error}")),
        signal = terminated() => {
            info!(%signal, "shutting down");
            Ok(())
        }
    };

    #[cfg(feature = "fork")]
    state.forks.shutdown().await;

    outcome
}

/// Resolves on the first termination signal. On a platform without them it
/// never resolves, which leaves the client disconnecting as the only exit.
async fn terminated() -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut interrupt = match signal(SignalKind::interrupt()) {
            Ok(stream) => stream,
            Err(_) => return std::future::pending().await,
        };
        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(stream) => stream,
            Err(_) => return std::future::pending().await,
        };
        tokio::select! {
            _ = interrupt.recv() => "SIGINT",
            _ = terminate.recv() => "SIGTERM",
        }
    }
    #[cfg(not(unix))]
    {
        match tokio::signal::ctrl_c().await {
            Ok(()) => "ctrl-c",
            Err(_) => std::future::pending().await,
        }
    }
}
