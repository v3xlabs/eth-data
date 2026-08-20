# Tool reference

Generated from the compiled tool schemas by `just docs`. Do not edit by hand.

41 tools, with every feature enabled. A default build has one fewer: `avatar_get` needs the off-by-default `avatars` feature.

- [`abi_decode`](#abi_decode)
- [`address_get`](#address_get)
- [`address_nfts`](#address_nfts)
- [`address_tokens`](#address_tokens)
- [`address_transfers`](#address_transfers)
- [`address_txs`](#address_txs)
- [`avatar_get`](#avatar_get)
- [`balance_get`](#balance_get)
- [`block_get`](#block_get)
- [`call`](#call)
- [`calldata_encode`](#calldata_encode)
- [`code_get`](#code_get)
- [`contract_abi`](#contract_abi)
- [`contract_source`](#contract_source)
- [`ens_records`](#ens_records)
- [`ens_resolve`](#ens_resolve)
- [`ens_reverse`](#ens_reverse)
- [`fork_accounts`](#fork_accounts)
- [`fork_close`](#fork_close)
- [`fork_create`](#fork_create)
- [`fork_list`](#fork_list)
- [`fork_logs`](#fork_logs)
- [`fork_status`](#fork_status)
- [`gas_estimate`](#gas_estimate)
- [`icon_get`](#icon_get)
- [`logs_get`](#logs_get)
- [`network_list`](#network_list)
- [`network_status`](#network_status)
- [`nft_get`](#nft_get)
- [`nft_transfers`](#nft_transfers)
- [`nonce_get`](#nonce_get)
- [`price_get`](#price_get)
- [`price_route`](#price_route)
- [`receipt_get`](#receipt_get)
- [`signature_lookup`](#signature_lookup)
- [`storage_get`](#storage_get)
- [`token_get`](#token_get)
- [`token_holders`](#token_holders)
- [`trace_call`](#trace_call)
- [`trace_tx`](#trace_tx)
- [`tx_get`](#tx_get)

## `abi_decode`

Decode ABI data against a signature. Runs locally against no network.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `data` | string | yes | The 0x-prefixed data to decode. |
| `input_data` | boolean | no | Decode the function's inputs rather than its outputs. Set this when `data` is transaction calldata. Defaults to false. |
| `signature` | string | yes | A function signature such as `balanceOf(address)`, or a bare type list such as `(uint256,address)`. |

## `address_get`

Read what the explorer knows about an address: whether it is a contract, whether its source is verified, its tags, and its balance. This is indexed data. It is good for orientation and should not be the basis of a decision that a chain read could settle.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `address` | string | yes | An address, or an ENS name when the `ens` feature is compiled in. |
| `network` | string | yes | A name from `network_list` whose `capabilities.explorer` is true. |

## `address_nfts`

One bounded page of the NFTs an address holds.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `address` | string | yes | An address, or an ENS name when the `ens` feature is compiled in. |
| `cursor` | string | no | A `next_cursor` from a previous call. |
| `network` | string | yes | A name from `network_list` whose `capabilities.explorer` is true. |
| `page_size` | integer | no | Items per page, 1 to 100. Defaults to 50. |

## `address_tokens`

List an address's token balances, discovered through the explorer and verified on chain. The token list comes from the indexer, because the chain cannot be asked "which tokens has this address held" in bounded time. Every balance is then re-read through Multicall3 at one pinned block, so the numbers are authoritative and mutually consistent. A row where the two disagree is flagged rather than overwritten.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `address` | string | yes | An address, or an ENS name when the `ens` feature is compiled in. |
| `cursor` | string | no | A `next_cursor` from a previous call. |
| `include_icons` | boolean | no | Include icon candidate URLs. Costs nothing: the URLs are constructed, not fetched. Defaults to true. |
| `network` | string | yes | A name from `network_list` whose `capabilities.explorer` is true. |
| `page_size` | integer | no | How many tokens to return, 1 to 100. Defaults to 50. |
| `skip_verification` | boolean | no | Skip the on-chain verification pass and return the indexer's numbers. Faster, and the balances may be stale. Defaults to false. |
| `token_types` | array | no | Which token standards to include. Defaults to ERC-20 only, because a wallet with thousands of airdropped NFTs is what `address_nfts` is for. |

## `address_transfers`

One bounded page of an address's token transfers, newest first.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `address` | string | yes | An address, or an ENS name when the `ens` feature is compiled in. |
| `cursor` | string | no | A `next_cursor` from a previous call. |
| `network` | string | yes | A name from `network_list` whose `capabilities.explorer` is true. |
| `page_size` | integer | no | Items per page, 1 to 100. Defaults to 50. |
| `token` | string | no | Restrict to transfers of this token contract. |

## `address_txs`

One bounded page of an address's transactions, newest first.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `address` | string | yes | An address, or an ENS name when the `ens` feature is compiled in. |
| `cursor` | string | no | A `next_cursor` from a previous call. |
| `network` | string | yes | A name from `network_list` whose `capabilities.explorer` is true. |
| `page_size` | integer | no | Items per page, 1 to 100. Defaults to 50. |

## `avatar_get`

Resolve an ENS avatar record to a fetchable URL. Not implemented yet. The schema is fixed; the resolver is waiting on `eth-avatars` to expose one.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `name_or_address` | string | yes | An ENS name, or an address to reverse-resolve first. |
| `network` | string | yes | A name from `network_list` whose endpoint serves Ethereum mainnet. |

## `balance_get`

Read a native balance for an address or ENS name.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `address` | string | yes | A 20-byte address, or an ENS name when the `ens` feature is compiled in. |
| `block` | string | no | Block number, tag, or hash. Defaults to `latest`. |
| `network` | string | yes | A name from `network_list`. |

## `block_get`

Read one block by number, tag, or hash.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `block` | string | no | Block number, a tag (`latest`, `earliest`, `pending`, `safe`, `finalized`), or a 32-byte block hash. Defaults to `latest`. |
| `include_transactions` | boolean | no | Return full transaction objects instead of hashes. Defaults to false, because a full mainnet block is large. |
| `network` | string | yes | A name from `network_list`. |

## `call`

Execute a read-only call. This never broadcasts and never changes state, on this or any other network.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `block` | string | no | Block number, tag, or hash. Defaults to `latest`. |
| `data` | string | no | ABI-encoded calldata. Build it with `calldata_encode`. Defaults to `0x`. |
| `from` | string | no | The caller to simulate as. Defaults to the zero address. |
| `network` | string | yes | A name from `network_list`. |
| `to` | string | yes | The contract to call. |
| `value` | string | no | Native value to attach, decimal or 0x-prefixed wei. |

## `calldata_encode`

Encode a function signature and its arguments into calldata. Runs locally against no network.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `arguments` | array | no | One string per parameter, in order. Numbers may be decimal or 0x-prefixed; addresses and bytes are 0x-prefixed; booleans are `true` or `false`; arrays are `[a,b,c]`. |
| `signature` | string | yes | A Solidity function signature, for example `transfer(address,uint256)`. |

## `code_get`

Read deployed runtime bytecode.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `address` | string | yes | A 20-byte address. |
| `block` | string | no | Block number, tag, or hash. Defaults to `latest`. |
| `network` | string | yes | A name from `network_list`. |

## `contract_abi`

Read a verified contract's ABI.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `address` | string | yes | An address, or an ENS name when the `ens` feature is compiled in. |
| `network` | string | yes | A name from `network_list` whose `capabilities.explorer` is true. |

## `contract_source`

Read a verified contract's source metadata, and optionally its source.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `address` | string | yes | The contract address. |
| `include_source` | boolean | no | Include the full source text. Off by default because a verified contract's sources can be hundreds of kilobytes. |
| `network` | string | yes | A name from `network_list` whose `capabilities.explorer` is true. |

## `ens_records`

Read the standard ENS text records for a name.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `keys` | array | no | Extra text record keys to read alongside the standard set. |
| `name` | string | yes | An ENS name, for example `vitalik.eth`. |
| `network` | string | yes | A name from `network_list` whose endpoint serves Ethereum mainnet. |

## `ens_resolve`

Resolve an ENS name to an address.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | string | yes | An ENS name, for example `vitalik.eth`. |
| `network` | string | yes | A name from `network_list` whose endpoint serves Ethereum mainnet. |

## `ens_reverse`

Resolve an address to its primary ENS name, and check that the name resolves back to the same address before reporting it as verified.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `address` | string | yes | A 20-byte address. |
| `network` | string | yes | A name from `network_list` whose endpoint serves Ethereum mainnet. |

## `fork_accounts`

List the fork's funded development accounts and their keys.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `fork_id` | string | yes | An id from `fork_create` or `fork_list`. |

## `fork_close`

Stop a fork and release its port.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `fork_id` | string | yes | An id from `fork_create` or `fork_list`. |

## `fork_create`

Start an isolated Anvil fork of a configured network and return the loopback RPC URL to drive it at. Everything you do to the fork afterwards goes through your own tools against that URL: `cast`, `forge`, or a script in the repository. There are deliberately no tools here for setting balances, impersonating, or sending, because `cast` already does all three with better arguments.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `block_number` | integer | no | Fork at this height instead of the chain head. Pin it when you want a run to be reproducible. |
| `network` | string | yes | A name from `network_list`. Its RPC URL becomes the fork source. |

## `fork_list`

List every fork this server is running, with the URL for each.

Takes no arguments.

## `fork_logs`

Read the tail of the fork's Anvil output. Anvil logs every RPC call it serves, so this is where you see what your own `cast` and `forge` commands did against the fork, and why one of them failed.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `fork_id` | string | yes | An id from `fork_create` or `fork_list`. |
| `lines` | integer | no | How many lines from the end to return. Defaults to 50, capped at 200. |

## `fork_status`

Check whether a fork is still alive and where its head is.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `fork_id` | string | yes | An id from `fork_create` or `fork_list`. |

## `gas_estimate`

Estimate the gas a transaction would use, without broadcasting it.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `data` | string | no | ABI-encoded calldata, or deployment bytecode when `to` is omitted. |
| `from` | string | no | The sender to simulate as. |
| `network` | string | yes | A name from `network_list`. |
| `to` | string | no | The contract to call. Omit to estimate a deployment. |
| `value` | string | no | Native value to attach, decimal or 0x-prefixed wei. |

## `icon_get`

Resolve icon candidate URLs for a network, its native asset, or an ERC-20. Returns URLs and their sources. It does not download any image, so the call is free and no image bytes enter the conversation.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `kind` | object | no | What to fetch an icon for. Defaults to `erc20` when `token` is given, and `network` when it is not. |
| `network` | string | yes | A name from `network_list`. Its chain ID selects the icon set. |
| `token` | string | no | The ERC-20 contract address. Required when `kind` is `erc20`. |

## `logs_get`

Read logs over a bounded block range.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `address` | string | no | Restrict to logs emitted by this contract. |
| `from_block` | integer | yes | First block in the range, inclusive. |
| `network` | string | yes | A name from `network_list`. |
| `to_block` | integer | yes | Last block in the range, inclusive. At most 10,000 blocks after `from_block`; a wider range is rejected before anything is sent. |
| `topics` | array | no | Up to four topic filters, positionally. Use `null` for a wildcard in a position you do not want to constrain. Topic 0 is the event signature hash; `signature_lookup` can tell you what one is. |

## `network_list`

List configured networks, the chain each endpoint resolved to, and what that chain unlocked. Start here: it is the one place that explains why a tool would answer `unsupported_on_network`.

Takes no arguments.

## `network_status`

Chain identity, client version, head block, and gas price for one network.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `network` | string | yes | A name from `network_list`. |

## `nft_get`

Read one NFT: its owner, its metadata URI, and whatever metadata the explorer has already resolved.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `contract` | string | yes | The collection contract address. |
| `network` | string | yes | A name from `network_list` whose `capabilities.explorer` is true. |
| `token_id` | string | yes | The token id, as a decimal string. |

## `nft_transfers`

One bounded page of transfers for a collection, or for one token in it.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `contract` | string | yes | The collection contract address. |
| `cursor` | string | no | A `next_cursor` from a previous call. |
| `network` | string | yes | A name from `network_list` whose `capabilities.explorer` is true. |
| `page_size` | integer | no | Items per page, 1 to 100. Defaults to 50. |
| `token_id` | string | no | Restrict to one token id. Omit for the whole collection. |

## `nonce_get`

Read how many transactions an address has sent.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `address` | string | yes | A 20-byte address. |
| `block` | string | no | Block number, tag, or hash. Defaults to `latest`. |
| `network` | string | yes | A name from `network_list`. |

## `price_get`

Price one asset in another, read from on-chain pools and feeds. Routes through Uniswap v2 and v3, Chainlink, and ERC-4626 vaults, ending at a stablecoin pegged to `fiat:usd`. The route and the block are part of the answer: check them before trusting the number.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `asset` | string | yes | An ERC-20 address, `native` for the chain's own currency, or `fiat:<symbol>`. |
| `block` | integer | no | Price at this block instead of the head, for a reproducible number. |
| `network` | string | yes | A name from `network_list` whose `capabilities.prices` is true. |
| `quote` | string | no | What to price it in. An ERC-20 address, `native`, or `fiat:usd`. Defaults to `fiat:usd`. |

## `price_route`

Show the route a price would take, without quoting it. Use this when `price_get` says there is no route, or when a price looks wrong and you want to see which pools produced it.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `asset` | string | yes | An ERC-20 address, `native`, or `fiat:<symbol>`. |
| `network` | string | yes | A name from `network_list` whose `capabilities.prices` is true. |
| `quote` | string | no | What to route towards. Defaults to `fiat:usd`. |

## `receipt_get`

Read one transaction receipt by hash. `summary.succeeded` is the field worth reading first: a mined transaction can still have reverted.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `network` | string | yes | A name from `network_list`. |
| `transaction_hash` | string | yes | 32-byte transaction hash. |

## `signature_lookup`

Look up the signatures that hash to a selector or an event topic.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `selector` | string | yes | A 4-byte function selector such as `0xa9059cbb`, a 32-byte event topic, or full calldata whose first four bytes are the selector. |

## `storage_get`

Read one contract storage slot, and the two readings of that word that are almost always what the caller wanted.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `address` | string | yes | A 20-byte contract address. |
| `block` | string | no | Block number, tag, or hash. Defaults to `latest`. |
| `network` | string | yes | A name from `network_list`. |
| `slot` | string | yes | Slot index, decimal or 0x-prefixed. |

## `token_get`

Read an ERC-20's metadata, from the contract first and the explorer second.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `address` | string | yes | The token contract address. |
| `network` | string | yes | A name from `network_list`. |

## `token_holders`

One bounded page of a token's holders, largest first.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `address` | string | yes | An address, or an ENS name when the `ens` feature is compiled in. |
| `cursor` | string | no | A `next_cursor` from a previous call. |
| `network` | string | yes | A name from `network_list` whose `capabilities.explorer` is true. |
| `page_size` | integer | no | Items per page, 1 to 100. Defaults to 50. |

## `trace_call`

Trace a read-only call without sending it.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `block` | string | no | Block number, tag, or hash. Defaults to `latest`. |
| `data` | string | no | ABI-encoded calldata. |
| `from` | string | no | The caller to simulate as. |
| `network` | string | yes | A name from `network_list`. Check `capabilities.traces` first. |
| `to` | string | yes | The contract to call. |
| `value` | string | no | Native value to attach, decimal or 0x-prefixed wei. |

## `trace_tx`

Trace one executed transaction. Requires an endpoint that serves the `trace_` or `debug_` namespace; `network_list` reports which do.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `network` | string | yes | A name from `network_list`. Check `capabilities.traces` first. |
| `transaction_hash` | string | yes | 32-byte transaction hash. |

## `tx_get`

Read one transaction by hash. A hash the node has never seen returns `found: false` rather than an error, because "not present" is an answer.

| Argument | Type | Required | Description |
| --- | --- | --- | --- |
| `network` | string | yes | A name from `network_list`. |
| `transaction_hash` | string | yes | 32-byte transaction hash. |

