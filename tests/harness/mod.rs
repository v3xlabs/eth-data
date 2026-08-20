//! Driving the compiled binary the way an MCP client does: a child process,
//! JSON-RPC over its stdio, and nothing else on stdout.
//!
// Each integration test binary compiles this module separately, so a helper only
// one of them needs reads as dead code in the others.
#![allow(dead_code)]

use std::{process::Stdio, time::Duration};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
};

pub const READY: Duration = Duration::from_secs(60);

pub struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: i64,
}

impl Server {
    /// Starts the binary against exactly the networks given, which replaces the
    /// shipped defaults rather than adding to them.
    pub async fn start(networks: &[(&str, &str)]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_eth-data-mcp"));
        for (name, url) in networks {
            command.env(format!("ETH_DATA_NETWORKS__{}", name.to_uppercase()), url);
        }

        let mut child = command
            .env("ETH_DATA_LOG", "warn")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .expect("the test binary was built by cargo");

        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = BufReader::new(child.stdout.take().expect("stdout was piped")).lines();

        let mut server = Self {
            child,
            stdin,
            stdout,
            next_id: 0,
        };
        server.initialize().await;
        server
    }

    async fn send(&mut self, message: &Value) {
        let line = format!("{}\n", serde_json::to_string(message).unwrap());
        self.stdin.write_all(line.as_bytes()).await.unwrap();
        self.stdin.flush().await.unwrap();
    }

    async fn read(&mut self) -> Value {
        let line = tokio::time::timeout(READY, self.stdout.next_line())
            .await
            .expect("the server answered within the timeout")
            .expect("stdout is readable")
            .expect("the server did not close stdout");
        serde_json::from_str(&line).unwrap_or_else(|error| {
            panic!("stdout carried non-protocol output: {line:?} ({error})")
        })
    }

    pub async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .await;

        loop {
            let message = self.read().await;
            // Notifications and unrelated traffic are skipped, not asserted on.
            if message.get("id") == Some(&json!(id)) {
                return message;
            }
        }
    }

    async fn initialize(&mut self) {
        let response = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "eth-data-tests", "version": "0" },
                }),
            )
            .await;
        assert!(
            response.get("result").is_some(),
            "initialize failed: {response}"
        );
        self.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await;
    }

    pub async fn tool_names(&mut self) -> Vec<String> {
        let response = self.request("tools/list", json!({})).await;
        response["result"]["tools"]
            .as_array()
            .expect("tools/list returns an array")
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect()
    }

    /// Returns the parsed structured content of a tool call, or its error body.
    pub async fn call(&mut self, name: &str, arguments: Value) -> ToolOutcome {
        let response = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await;

        let result = &response["result"];
        let is_error = result["isError"].as_bool().unwrap_or(false);
        let body = result
            .get("structuredContent")
            .cloned()
            .filter(|value| !value.is_null())
            .or_else(|| {
                let text = result["content"][0]["text"].as_str()?;
                serde_json::from_str(text).ok()
            })
            .unwrap_or_else(|| response.clone());

        if is_error {
            ToolOutcome::Error(body)
        } else {
            ToolOutcome::Ok(body)
        }
    }

    pub async fn shutdown(mut self) {
        let _ = self.child.kill().await;
    }
}

#[derive(Debug)]
pub enum ToolOutcome {
    Ok(Value),
    Error(Value),
}

impl ToolOutcome {
    pub fn unwrap(self) -> Value {
        match self {
            Self::Ok(value) => value,
            Self::Error(value) => panic!("the tool failed: {value}"),
        }
    }

    pub fn unwrap_err(self) -> Value {
        match self {
            Self::Error(value) => value,
            Self::Ok(value) => panic!("the tool unexpectedly succeeded: {value}"),
        }
    }
}

/// A throwaway chain to point the server at, so no test needs the internet.
pub struct LocalChain {
    child: Child,
    pub url: String,
}

impl LocalChain {
    pub async fn start() -> Self {
        let mut child = Command::new("anvil")
            .args(["--host", "127.0.0.1", "--port", "0"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("anvil is on PATH inside `nix develop`");

        let mut lines = BufReader::new(child.stdout.take().expect("stdout was piped")).lines();
        let port = tokio::time::timeout(READY, async {
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(tail) = line.rsplit_once("Listening on ")
                    && let Some((_, port)) = tail.1.rsplit_once(':')
                    && let Ok(port) = port.trim().parse::<u16>()
                {
                    return port;
                }
            }
            panic!("anvil exited before it listened");
        })
        .await
        .expect("anvil started within the timeout");

        // Keep draining so anvil never blocks on a full pipe.
        tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });

        Self {
            child,
            url: format!("http://127.0.0.1:{port}"),
        }
    }

    pub async fn shutdown(mut self) {
        let _ = self.child.kill().await;
    }
}
