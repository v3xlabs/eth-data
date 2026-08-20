//! Starting an Anvil child, and finding out where it landed.
//!
//! Anvil binds port zero itself and reports the port it got. The Python server
//! bound port zero from Python, closed the socket, and passed the number on,
//! which left a window for anything else on the machine to take it.

use std::{
    collections::VecDeque,
    process::Stdio,
    sync::{Arc, Mutex},
};

use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::oneshot,
};
use tracing::debug;

use crate::{
    limits::{FORK_LOG_LINES, FORK_READY_TIMEOUT},
    server::error::ToolError,
};

/// A bounded tail of the child's output. Anvil logs every RPC call it serves,
/// which is what makes this the record of what the caller's own `cast` and
/// `forge` commands did.
#[derive(Clone, Default)]
pub struct LogBuffer(Arc<Mutex<VecDeque<String>>>);

impl LogBuffer {
    fn push(&self, line: String) {
        let mut lines = self
            .0
            .lock()
            .expect("the log buffer lock is never poisoned");
        if lines.len() == FORK_LOG_LINES {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    pub fn tail(&self, count: usize) -> Vec<String> {
        let lines = self
            .0
            .lock()
            .expect("the log buffer lock is never poisoned");
        lines
            .iter()
            .skip(lines.len().saturating_sub(count))
            .cloned()
            .collect()
    }
}

pub struct Spawned {
    pub child: Child,
    pub port: u16,
    /// The development keys Anvil printed, in account order, so the caller can
    /// paste one straight into `--private-key`.
    pub private_keys: Vec<String>,
    pub logs: LogBuffer,
}

/// `Listening on 127.0.0.1:41337`
fn parse_port(line: &str) -> Option<u16> {
    let tail = line.rsplit_once("Listening on ")?.1;
    tail.rsplit_once(':')?.1.trim().parse().ok()
}

/// `(0) 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80`
fn parse_private_key(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with('(') {
        return None;
    }
    let candidate = trimmed.rsplit(')').next()?.trim();
    let hex = candidate.strip_prefix("0x")?;
    (hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit())).then(|| candidate.to_string())
}

/// Ask the kernel to signal the child when this process dies.
///
/// `kill_on_drop` covers a clean exit, and the shutdown path covers SIGTERM.
/// Neither covers SIGKILL, which an editor closing a stuck MCP server does use,
/// and an orphaned Anvil holds its port and its forked state until the machine
/// reboots. This is the same guarantee bubblewrap's `--die-with-parent` gives.
#[cfg(target_os = "linux")]
fn die_with_parent(command: &mut Command) {
    // SAFETY: prctl is async-signal-safe and touches only this process's own
    // death signal, which is what pre_exec requires.
    unsafe {
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            // The parent may already have died in the window between fork and
            // here, in which case nothing will ever deliver that signal.
            if libc::getppid() == 1 {
                libc::_exit(1);
            }
            Ok(())
        });
    }
}

/// No portable equivalent exists elsewhere. A fork survives a SIGKILLed server
/// on other platforms, and `fork_list` on the next server will not know about
/// it. Documented in docs/configuration.md rather than papered over.
#[cfg(not(target_os = "linux"))]
fn die_with_parent(_command: &mut Command) {}

pub async fn spawn(fork_url: &str, fork_block: Option<u64>) -> Result<Spawned, ToolError> {
    let mut command = Command::new("anvil");
    command
        .arg("--host")
        .arg("127.0.0.1")
        // Port zero, so Anvil binds it and there is no window to lose it in.
        .arg("--port")
        .arg("0")
        .arg("--fork-url")
        .arg(fork_url);
    if let Some(block) = fork_block {
        command.arg("--fork-block-number").arg(block.to_string());
    }

    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    die_with_parent(&mut command);

    let mut child = command.spawn().map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => {
            ToolError::fork("`anvil` is not on PATH, so no fork can be started.".to_string())
                .with_remedy(
                    "Install Foundry, or run the `eth-data-mcp` package from the flake, \
                 which wraps anvil onto PATH."
                        .to_string(),
                )
        }
        _ => ToolError::fork(format!("Could not start anvil: {error}")),
    })?;

    let logs = LogBuffer::default();
    let keys = Arc::new(Mutex::new(Vec::new()));
    let (ready_tx, ready_rx) = oneshot::channel::<u16>();

    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");

    tokio::spawn({
        let logs = logs.clone();
        let keys = Arc::clone(&keys);
        async move {
            let mut lines = BufReader::new(stdout).lines();
            let mut ready_tx = Some(ready_tx);
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(key) = parse_private_key(&line) {
                    keys.lock()
                        .expect("the key lock is never poisoned")
                        .push(key);
                }
                if let Some(port) = parse_port(&line)
                    && let Some(sender) = ready_tx.take()
                {
                    let _ = sender.send(port);
                }
                logs.push(line);
            }
        }
    });

    tokio::spawn({
        let logs = logs.clone();
        async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                logs.push(format!("stderr: {line}"));
            }
        }
    });

    let port = match tokio::time::timeout(FORK_READY_TIMEOUT, ready_rx).await {
        Ok(Ok(port)) => port,
        // The channel closing means the reader saw EOF: the child exited
        // before it ever listened. Its own output says why.
        Ok(Err(_)) | Err(_) => {
            let _ = child.kill().await;
            let tail = logs.tail(20).join("\n");
            return Err(ToolError::fork(format!(
                "Anvil did not start listening within {} seconds.",
                FORK_READY_TIMEOUT.as_secs()
            ))
            .with_remedy(if tail.is_empty() {
                "It produced no output.".to_string()
            } else {
                format!("Its last output was:\n{tail}")
            }));
        }
    };

    let private_keys = keys.lock().expect("the key lock is never poisoned").clone();
    debug!(port, keys = private_keys.len(), "anvil is listening");

    Ok(Spawned {
        child,
        port,
        private_keys,
        logs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_port_anvil_chose() {
        assert_eq!(parse_port("Listening on 127.0.0.1:41337"), Some(41337));
        assert_eq!(parse_port("Listening on 0.0.0.0:8545"), Some(8545));
        assert_eq!(parse_port("Available Accounts"), None);
    }

    #[test]
    fn finds_private_keys_in_the_banner() {
        let line = "(0) 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
        assert_eq!(
            parse_private_key(line).as_deref(),
            Some("0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
        );
    }

    #[test]
    fn does_not_mistake_an_account_line_for_a_key() {
        // Addresses are 20 bytes, keys are 32. Only the key length matches.
        let line = "(0) 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266 (10000.000000000000000000 ETH)";
        assert_eq!(parse_private_key(line), None);
    }

    #[test]
    fn the_log_buffer_keeps_the_tail_and_drops_the_head() {
        let logs = LogBuffer::default();
        for index in 0..FORK_LOG_LINES + 10 {
            logs.push(index.to_string());
        }
        let tail = logs.tail(3);
        assert_eq!(tail, vec!["507", "508", "509"]);
        assert_eq!(logs.tail(usize::MAX).len(), FORK_LOG_LINES);
    }
}
