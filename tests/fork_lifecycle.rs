//! The fork handover, proved the way a caller experiences it.
//!
//! The point of `fork_create` is the URL it returns. A test that only asks the
//! server about its own fork proves nothing, so this drives the fork from a
//! separate `cast` process, which is exactly what the caller is told to do.

mod harness;

use std::process::Stdio;

use harness::{LocalChain, Server};
use serde_json::json;
use tokio::process::Command;

async fn cast(args: &[&str]) -> Result<String, String> {
    let output = Command::new("cast")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| format!("could not run cast: {error}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[tokio::test]
async fn a_fork_is_reachable_from_another_process_and_dies_with_the_server() {
    let chain = LocalChain::start().await;
    let mut server = Server::start(&[("local", &chain.url)]).await;

    let created = server
        .call("fork_create", json!({ "network": "local" }))
        .await
        .unwrap();

    let rpc_url = created["rpc_url"].as_str().unwrap().to_string();
    let fork_id = created["fork_id"].as_str().unwrap().to_string();
    assert!(created["port"].as_u64().unwrap() > 0);
    assert!(
        created["how_to_use"].as_str().unwrap().contains("cast"),
        "the handover has to tell the caller what to do with the URL",
    );

    let accounts = created["accounts"].as_array().unwrap();
    assert!(
        !accounts.is_empty(),
        "a fork must come with funded accounts"
    );
    let account = accounts[0]["address"].as_str().unwrap().to_string();
    assert!(
        accounts[0]["private_key"].as_str().is_some(),
        "the key is what makes the account usable from a shell",
    );

    // The whole design rests on this line: a different process can reach it.
    let head = cast(&["block-number", "--rpc-url", &rpc_url])
        .await
        .expect("cast reached the fork");
    assert!(head.parse::<u64>().is_ok(), "cast returned {head:?}");

    // And the retired tools really are available as cast commands.
    let before = cast(&["balance", &account, "--rpc-url", &rpc_url])
        .await
        .unwrap();
    cast(&[
        "rpc",
        "anvil_setBalance",
        &account,
        "0xde0b6b3a7640000",
        "--rpc-url",
        &rpc_url,
    ])
    .await
    .expect("anvil_setBalance through cast");
    let after = cast(&["balance", &account, "--rpc-url", &rpc_url])
        .await
        .unwrap();
    assert_ne!(before, after, "the cheat code did not take effect");
    assert_eq!(after, "1000000000000000000");

    // Anvil logs every call it serves, so fork_logs is the record of the above.
    let logs = server
        .call("fork_logs", json!({ "fork_id": fork_id, "lines": 100 }))
        .await
        .unwrap();
    let lines = logs["lines"].as_array().unwrap();
    assert!(
        lines
            .iter()
            .any(|line| line.as_str().unwrap_or("").contains("anvil_setBalance")),
        "fork_logs should show what cast did: {lines:?}",
    );

    let status = server
        .call("fork_status", json!({ "fork_id": &fork_id }))
        .await
        .unwrap();
    assert_eq!(status["alive"], true);

    let listed = server.call("fork_list", json!({})).await.unwrap();
    assert_eq!(listed["forks"].as_array().unwrap().len(), 1);

    server
        .call("fork_close", json!({ "fork_id": &fork_id }))
        .await
        .unwrap();

    assert!(
        cast(&["block-number", "--rpc-url", &rpc_url])
            .await
            .is_err(),
        "the fork was still reachable after fork_close",
    );

    let error = server
        .call("fork_status", json!({ "fork_id": &fork_id }))
        .await
        .unwrap_err();
    assert_eq!(error["category"], "fork");

    server.shutdown().await;
    chain.shutdown().await;
}

#[tokio::test]
async fn two_forks_of_one_network_do_not_share_state() {
    let chain = LocalChain::start().await;
    let mut server = Server::start(&[("local", &chain.url)]).await;

    let first = server
        .call("fork_create", json!({ "network": "local" }))
        .await
        .unwrap();
    let second = server
        .call("fork_create", json!({ "network": "local" }))
        .await
        .unwrap();

    let first_url = first["rpc_url"].as_str().unwrap().to_string();
    let second_url = second["rpc_url"].as_str().unwrap().to_string();
    assert_ne!(first_url, second_url);

    let victim = "0x000000000000000000000000000000000000dEaD";
    cast(&[
        "rpc",
        "anvil_setBalance",
        victim,
        "0x1234",
        "--rpc-url",
        &first_url,
    ])
    .await
    .unwrap();

    let touched = cast(&["balance", victim, "--rpc-url", &first_url])
        .await
        .unwrap();
    let untouched = cast(&["balance", victim, "--rpc-url", &second_url])
        .await
        .unwrap();

    assert_eq!(touched, "4660");
    assert_eq!(untouched, "0", "the second fork saw the first fork's state");

    server.shutdown().await;
    chain.shutdown().await;
}

#[tokio::test]
async fn killing_the_server_reaps_its_forks() {
    let chain = LocalChain::start().await;
    let mut server = Server::start(&[("local", &chain.url)]).await;

    let created = server
        .call("fork_create", json!({ "network": "local" }))
        .await
        .unwrap();
    let rpc_url = created["rpc_url"].as_str().unwrap().to_string();

    assert!(cast(&["block-number", "--rpc-url", &rpc_url]).await.is_ok());

    server.shutdown().await;

    // An orphaned anvil would hold its port and a few hundred megabytes of
    // forked state for as long as the machine is up.
    let mut reaped = false;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        if cast(&["block-number", "--rpc-url", &rpc_url])
            .await
            .is_err()
        {
            reaped = true;
            break;
        }
    }
    assert!(reaped, "the fork outlived the server that started it");

    chain.shutdown().await;
}
