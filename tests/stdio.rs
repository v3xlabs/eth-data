//! The MCP surface, exercised through the compiled binary over stdio.

mod harness;

use harness::{LocalChain, Server};
use serde_json::json;

#[tokio::test]
async fn initializes_lists_tools_and_answers_a_call() {
    let chain = LocalChain::start().await;
    let mut server = Server::start(&[("local", &chain.url)]).await;

    let names = server.tool_names().await;
    assert!(names.contains(&"network_list".to_string()));
    assert!(names.contains(&"block_get".to_string()));
    assert!(names.contains(&"calldata_encode".to_string()));

    // Tool names are unique and sorted, which is what makes a client's list
    // stable between runs.
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted, names, "tool names are not sorted and unique");

    let listed = server.call("network_list", json!({})).await.unwrap();
    let networks = listed["networks"].as_array().unwrap();
    assert_eq!(networks.len(), 1, "config replaced the defaults: {listed}");
    assert_eq!(networks[0]["network"], "local");
    assert_eq!(networks[0]["available"], true);
    assert_eq!(networks[0]["chain_id"], 31337);

    server.shutdown().await;
    chain.shutdown().await;
}

#[tokio::test]
async fn the_local_abi_codec_round_trips() {
    let chain = LocalChain::start().await;
    let mut server = Server::start(&[("local", &chain.url)]).await;

    let encoded = server
        .call(
            "calldata_encode",
            json!({
                "signature": "transfer(address,uint256)",
                "arguments": ["0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045", "1000000"],
            }),
        )
        .await
        .unwrap();

    assert_eq!(encoded["selector"], "0xa9059cbb");
    let calldata = encoded["calldata"].as_str().unwrap().to_string();

    let decoded = server
        .call(
            "abi_decode",
            json!({
                "signature": "transfer(address,uint256)",
                "data": calldata,
                "input_data": true,
            }),
        )
        .await
        .unwrap();

    assert_eq!(decoded["selector"], "0xa9059cbb");
    assert!(decoded.get("selector_mismatch").is_none());
    assert_eq!(
        decoded["values"][0]["value"],
        "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045"
    );
    // A uint256 stays a decimal string rather than becoming a JSON number.
    assert_eq!(decoded["values"][1]["value"], "1000000");

    server.shutdown().await;
    chain.shutdown().await;
}

#[tokio::test]
async fn bad_input_is_categorised_and_never_leaves_the_process() {
    let chain = LocalChain::start().await;
    let mut server = Server::start(&[("local", &chain.url)]).await;

    let error = server
        .call(
            "balance_get",
            json!({ "network": "local", "address": "0xnope" }),
        )
        .await
        .unwrap_err();
    assert_eq!(error["category"], "invalid_input");

    let error = server
        .call(
            "balance_get",
            json!({ "network": "mainnet", "address": "0x0000000000000000000000000000000000000000" }),
        )
        .await
        .unwrap_err();
    assert_eq!(error["category"], "unknown_network");
    assert!(
        error["remedy"].as_str().unwrap().contains("local"),
        "the error should name the networks that do exist: {error}"
    );

    // 10,001 blocks is one over the limit, and is refused before any request.
    let error = server
        .call(
            "logs_get",
            json!({ "network": "local", "from_block": 0, "to_block": 10_001 }),
        )
        .await
        .unwrap_err();
    assert_eq!(error["category"], "invalid_input");

    server.shutdown().await;
    chain.shutdown().await;
}

#[tokio::test]
async fn reads_a_block_and_an_account_from_the_chain() {
    let chain = LocalChain::start().await;
    let mut server = Server::start(&[("local", &chain.url)]).await;

    let block = server
        .call("block_get", json!({ "network": "local" }))
        .await
        .unwrap();
    assert_eq!(block["number"], 0);
    assert!(block["hash"].as_str().unwrap().starts_with("0x"));

    // Anvil funds this development account with 10,000 of the native asset.
    let balance = server
        .call(
            "balance_get",
            json!({
                "network": "local",
                "address": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
            }),
        )
        .await
        .unwrap();
    assert_eq!(balance["balance"]["formatted"], "10000");
    assert_eq!(balance["balance"]["raw"], "10000000000000000000000");

    server.shutdown().await;
    chain.shutdown().await;
}

#[tokio::test]
async fn a_capability_the_chain_lacks_is_reported_rather_than_failed() {
    let chain = LocalChain::start().await;
    let mut server = Server::start(&[("local", &chain.url)]).await;

    // Anvil's chain id is 31337, which is in no built-in table, so the explorer
    // and price groups have nothing to work with. They must say so.
    let listed = server.call("network_list", json!({})).await.unwrap();
    let capabilities = &listed["networks"][0]["capabilities"];
    assert_eq!(capabilities["explorer"], false);
    assert_eq!(capabilities["prices"], false);

    let error = server
        .call(
            "address_get",
            json!({
                "network": "local",
                "address": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(error["category"], "unsupported_on_network");

    server.shutdown().await;
    chain.shutdown().await;
}
