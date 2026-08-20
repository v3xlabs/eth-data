# Install

## Nix

```bash
nix run github:v3xlabs/eth-data
```

The flake's `eth-data-mcp` wraps the binary with `anvil` on `PATH`, so forks work out
of the box. A bare `nix run` that cannot create a fork is a broken first impression.

| Output | What it is |
| --- | --- |
| `packages.eth-data-mcp` | The binary, `anvil` wrapped onto `PATH`. Also `packages.default`. |
| `packages.eth-data-mcp-unwrapped` | No wrapper, for callers supplying their own Foundry. |
| `packages.eth-data-mcp-sandboxed` | Bubblewrap wrapper, Linux only. See [clients.md](clients.md). |
| `apps.default` | What `nix run` uses. |
| `overlays.default` | Adds `eth-data-mcp` to `pkgs`. |
| `devShells.default` | Rust toolchain, `bacon`, `just`, and Foundry. |

As a flake input:

```nix
{
  inputs.eth-data.url = "github:v3xlabs/eth-data";
}
```

```nix
environment.systemPackages = [
  inputs.eth-data.packages.${pkgs.stdenv.hostPlatform.system}.eth-data-mcp
];
```

## Cargo

```bash
cargo install eth-data-mcp
```

Fork tools need `anvil` on `PATH`. Install Foundry, or build without it:

```bash
cargo install eth-data-mcp --no-default-features --features traces,ens,explorer,signatures,icons,prices
```

## From source

```bash
git clone https://github.com/v3xlabs/eth-data
cd eth-data
nix develop
cargo build --release
```

## Verifying it works

```bash
eth-data-mcp --print-tools | jq length     # 40 with default features
eth-data-mcp --version
```

`--print-tools` makes no network calls and needs no configuration, so it is also the
fastest way to check an install.
