default:
    @just --list

build:
    cargo build --release

check:
    cargo fmt --check
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test

fmt:
    cargo fmt

# Every feature alone, plus the default set and none of them.
matrix:
    cargo check --no-default-features
    for f in traces ens explorer signatures icons prices fork; do \
        cargo check --no-default-features --features $f; \
    done
    cargo check

run:
    cargo run

tools:
    cargo run -- --print-tools

# Regenerate docs/tools.md and the schema snapshot from the compiled catalogue.
docs:
    #!/usr/bin/env bash
    cargo run --quiet --all-features -- --print-tools > tests/fixtures/tools.json
    set -euo pipefail
    {
      echo "# Tool reference"
      echo
      echo "Generated from the compiled tool schemas by \`just docs\`. Do not edit by hand."
      echo
      echo "$(jq length tests/fixtures/tools.json) tools, with every feature enabled. A default build has one fewer: \`avatar_get\` needs the off-by-default \`avatars\` feature."
      echo
      jq -r '.[] | "- [`\(.name)`](#\(.name))"' tests/fixtures/tools.json
      echo
      jq -r -f scripts/tools-doc.jq tests/fixtures/tools.json
    } > docs/tools.md
