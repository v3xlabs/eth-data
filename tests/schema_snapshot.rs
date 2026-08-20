//! The tool catalogue, locked.
//!
//! A tool's name, description, or argument shape changing is a change to a
//! public contract. This makes that show up as a reviewable diff instead of as
//! somebody's client breaking.
//!
//! Run `cargo run --all-features -- --print-tools > tests/fixtures/tools.json`
//! to accept a change on purpose.

use std::process::Stdio;

use serde_json::Value;
use tokio::process::Command;

const SNAPSHOT: &str = include_str!("fixtures/tools.json");

async fn catalogue() -> Vec<Value> {
    let output = Command::new(env!("CARGO_BIN_EXE_eth-data-mcp"))
        .arg("--print-tools")
        .stdin(Stdio::null())
        .output()
        .await
        .expect("the test binary was built by cargo");

    assert!(output.status.success(), "--print-tools failed");
    serde_json::from_slice(&output.stdout).expect("--print-tools emits JSON")
}

fn names(tools: &[Value]) -> Vec<&str> {
    tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn the_tool_catalogue_matches_the_snapshot() {
    let actual = catalogue().await;
    let expected: Vec<Value> = serde_json::from_str(SNAPSHOT).expect("the snapshot is JSON");

    let actual_names = names(&actual);
    let expected_names = names(&expected);
    assert_eq!(
        actual_names, expected_names,
        "the tool list changed. Accept it with:\n  \
         cargo run --all-features -- --print-tools > tests/fixtures/tools.json",
    );

    for (tool, snapshot) in actual.iter().zip(&expected) {
        assert_eq!(
            tool, snapshot,
            "the schema for {} changed. Accept it with:\n  \
             cargo run --all-features -- --print-tools > tests/fixtures/tools.json",
            tool["name"],
        );
    }
}

#[tokio::test]
async fn every_tool_is_documented_and_uniquely_named() {
    let tools = catalogue().await;

    let mut seen = std::collections::BTreeSet::new();
    for tool in &tools {
        let name = tool["name"].as_str().expect("every tool has a name");
        assert!(seen.insert(name), "{name} is registered twice");

        let description = tool["description"]
            .as_str()
            .unwrap_or_else(|| panic!("{name} has no description"));
        assert!(
            description.len() > 20,
            "{name}'s description is too short to be useful: {description:?}",
        );

        assert_eq!(
            tool["inputSchema"]["type"], "object",
            "{name} does not take an object",
        );
    }
}

/// `--print-tools` needs no configuration and no network, which is what makes
/// it usable in CI and for generating `docs/tools.md`.
#[tokio::test]
async fn printing_the_catalogue_needs_no_network() {
    let output = Command::new(env!("CARGO_BIN_EXE_eth-data-mcp"))
        .arg("--print-tools")
        // A network that cannot resolve. If the catalogue needed one, this
        // would hang or fail rather than print.
        .env("ETH_DATA_NETWORKS__BROKEN", "https://127.0.0.1:1/rpc")
        .stdin(Stdio::null())
        .output()
        .await
        .expect("the test binary was built by cargo");

    assert!(output.status.success());
    let tools: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!tools.is_empty());
}
