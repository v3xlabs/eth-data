# Clients

`eth-data-mcp` speaks MCP over stdio. Protocol goes to stdout, logs go to stderr,
and nothing else touches either.

## Claude Code

`~/.claude.json`, or `.mcp.json` in a project:

```json
{
  "mcpServers": {
    "eth_data": {
      "type": "stdio",
      "command": "eth-data-mcp",
      "args": []
    }
  }
}
```

## OpenCode

```json
{
  "mcp": {
    "eth_data": {
      "type": "local",
      "command": ["eth-data-mcp"],
      "enabled": true,
      "timeout": 30000
    }
  }
}
```

A 30 second timeout is worth setting. `fork_create` has to start Anvil and let it
backfill state from the fork source, which on mainnet takes a few seconds, and
`price_get` discovers pools on its first call for an asset.

## Any stdio client

```bash
eth-data-mcp                      # serve on stdio
eth-data-mcp --config ./eth.toml  # a specific config file
eth-data-mcp --print-tools        # the tool catalogue as JSON, no network needed
eth-data-mcp --version
```

## Environment

| Variable | Effect |
| --- | --- |
| `ETH_DATA_NETWORKS__<NAME>` | An RPC URL for network `<name>`. Replaces the shipped defaults. |
| `ETH_DATA_LOG` | A `tracing` filter, for example `debug` or `eth_data_mcp=debug`. Defaults to `info`. |

## Running it sandboxed

On Linux the flake also builds `eth-data-mcp-sandboxed`, which runs the server under
bubblewrap with a temporary workspace and an explicit environment.

It keeps `--share-net`, and it has to. `fork_create` binds Anvil to `127.0.0.1` and
hands you that URL to drive with `cast`. An unshared network namespace would keep the
sandbox's loopback private, so every fork would start correctly and then be
unreachable from your shell, which reads as Anvil failing rather than as a sandbox
setting.
