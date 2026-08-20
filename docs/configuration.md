# Configuration

One field per network: an RPC URL.

```toml
# ~/.config/eth-data/config.toml
[networks]
mainnet = "https://ethereum.reth.rs/rpc"
local   = "http://127.0.0.1:8545"
```

Or from the environment, which takes precedence over the file:

```bash
ETH_DATA_NETWORKS__MAINNET=https://my-node.internal/rpc
```

`http` is allowed so a node on loopback needs no certificate. Any other scheme is
rejected at startup, because an RPC URL is not a file and not a socket. Network names
are lowercase letters, digits, and hyphens.

## Defaults replace, they do not merge

`mainnet`, `sepolia`, and `hoodi` ship compiled in, so a bare `nix run` works before
any config exists. Naming **any** network of your own replaces all three.

That is deliberate. If the defaults were a base to merge onto, there would be no way
to express "I want mainnet and nothing else", and every user would carry two testnets
they never asked for. A config file is the complete list.

## Everything else is derived

One `eth_chainId` call per endpoint at startup, and the rest follows.

| Derived | How | Unlocks | When it is unknown |
| --- | --- | --- | --- |
| Chain ID | `eth_chainId` at startup | everything below | The network is marked unavailable and reported that way by `network_list`. It is retried on next use. |
| Explorer | Built-in chain ID to Blockscout table | the 11 `explorer` tools | Those tools return `unsupported_on_network`. Everything else still works. |
| Trace support | One probe per namespace at startup | `trace_tx`, `trace_call` | Reported by `network_list` rather than failing at call time. |
| Icon queries | The chain ID is an `eth-icons` `NetworkId` | `icon_get` | Never. Individual sources report themselves unsupported. |
| Price anchors | Built-in chain ID to wrapped-native and stablecoin table | `price_get`, `price_route` | Those tools return `unsupported_on_network`. |
| Fork source | The same RPC URL becomes Anvil's `--fork-url` | the 6 `fork` tools | Never. Any configured network can be forked. |

Chains with built-in entries: Ethereum mainnet, Sepolia, Hoodi, Base, OP Mainnet,
Arbitrum One, Gnosis, and Polygon. Testnets carry no price anchors on purpose: a
price routed through a testnet pool is a number with no meaning.

Adding a chain is a change to `src/config/derived.rs`, not to your config. That is
the point of deriving it: a tool that works on a new chain because someone added
three lines beats one that works because every user looked up three URLs.

## A network that is down

An endpoint that does not answer at startup does not stop the server. It is marked
unavailable, `network_list` reports it with the reason, and the next tool call
against it re-probes. Startup only fails when **no** configured network answered,
because a server with one working network is useful and a server that refuses to
start is not.

## Upstreams, and what each one sees

Keyless is not the same as private.

| Upstream | Used by | Key | What it sees |
| --- | --- | --- | --- |
| Your configured RPC endpoints | chain, ens, prices, fork | none | Every address, block, and contract you ask about, plus a fork's backfill reads. |
| Public Blockscout instances | explorer | none | Every address, token, and collection you look up. |
| Multicall3 `0xcA11bde05977b3631167028862bE2a173976CA11` | `address_tokens`, `token_get` | on chain | Nothing extra. It is a contract call over your own RPC endpoint. |
| Uniswap, Chainlink, ERC-4626 vaults | prices | on chain | Nothing extra, same reason. |
| European Central Bank daily rates | prices | none | Only that a rate was requested. |
| Blockscout, Avara, Zerion, Smoldapp, SafeWallet icon hosts | icons | none | Nothing. `icon_get` builds URLs and never requests them. |
| openchain.xyz | signatures | none | The selectors you decode. |

The shipped defaults are third-party endpoints. If that matters, point them at your
own node: it is one line, and it is also how you get `trace_tx` working.

## Limits

Enforced before anything leaves the process.

| Limit | Value |
| --- | --- |
| Response body | 1 MiB |
| Request timeout | 20s, connect 10s |
| Redirects | 3 |
| Log block range | 10,000 blocks |
| Page size | 1 to 100, default 50 |
| Cursor length | 1,000 characters |
| ABI signature | 1,000 characters, 64 arguments |
| Multicall batch | 100 |
| Live forks | 8 |
| Fork startup | 30s |
| Fork log buffer | 500 lines, 200 per request |

A provider may be stricter than eth-data. `logs_get` says so explicitly when the
endpoint refuses a range that is inside our own limit.

## Features

Compile-time, not runtime. All on by default except `avatars`.

```bash
cargo build --no-default-features                    # chain and ABI only
cargo build --no-default-features --features fork    # plus local forks
cargo build --features avatars                       # see below
```

A feature that is off contributes no tool schema and no dependency. `--no-default-features`
has no Anvil, no `eth-prices`, and no `eth-icons` in the tree.

`avatars` is off because it does not work yet. `eth-avatars` is published at 0.0.1
with `pub fn hello()` and no resolver, so `avatar_get` currently returns an
`unsupported_on_network` error explaining that. Its schema is fixed so that turning
it on later is not a breaking change. Use `ens_records` for the raw `avatar` text
record in the meantime.

## Fork cleanup on other platforms

On Linux every Anvil child carries `PR_SET_PDEATHSIG`, so it dies even if the server
is SIGKILLed. No portable equivalent exists elsewhere: on macOS a fork can outlive a
SIGKILLed server, and the next server will not know about it. Closing a client
normally, or sending SIGTERM, reaps cleanly on every platform.
