//! The only place in VRManager that spawns `adb`.
//!
//! Guarantees provided here so no caller has to re-implement them:
//!   * every invocation has a timeout,
//!   * every invocation is cancellable,
//!   * the child process is killed when a timeout/cancel fires,
//!   * stdout/stderr are captured (or streamed line-by-line),
//!   * a non-zero exit becomes a typed [`CoreError::AdbFailed`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::core::adb::parse::{self, AdbDeviceEntry};
use crate::core::cancel::CancelToken;
use crate::error::{CoreError, CoreResult};

/// Default budget for a quick control command (`devices`, `getprop`, ...).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
/// Budget for operations that move bytes over USB/Wi-Fi.
pub const LONG_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Which child pipe a streamed line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stream {
    Stdout,
    Stderr,
}

/// Type-erased child pipe, so stdout and stderr readers share one type.
type BoxedPipe = Box<dyn tokio::io::AsyncRead + Unpin + Send>;

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn success(&self) -> bool {
        self.status == Some(0)
    }

    /// adb frequently exits 0 while printing an error to stdout, so callers
    /// that care about semantics use this instead of the exit code alone.
    pub fn combined(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

#[derive(Debug, Clone)]
pub struct AdbClient {
    adb: PathBuf,
}

impl AdbClient {
    pub fn new(adb: impl Into<PathBuf>) -> Self {
        Self { adb: adb.into() }
    }

    pub fn path(&self) -> &Path {
        &self.adb
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(&self.adb);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // Never flash a console window on Windows for a background probe.
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            // tokio's Command exposes this inherently on Windows.
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        cmd
    }

    /// Run `adb <args>` and collect its output.
    pub async fn exec(
        &self,
        args: &[&str],
        timeout: Duration,
        cancel: &CancelToken,
    ) -> CoreResult<CommandOutput> {
        if cancel.is_cancelled() {
            return Err(CoreError::Cancelled);
        }

        tracing::debug!(target: "vrmanager::adb", args = ?args, "exec");

        let child = self.command(args).spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => CoreError::AdbNotFound,
            _ => CoreError::AdbSpawn(e.to_string()),
        })?;

        let wait = child.wait_with_output();
        tokio::pin!(wait);

        let output = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                // `kill_on_drop` handles the actual kill when `wait` is dropped.
                return Err(CoreError::Cancelled);
            }
            res = tokio::time::timeout(timeout, &mut wait) => match res {
                Err(_) => return Err(CoreError::Timeout(timeout)),
                Ok(Err(e)) => return Err(CoreError::AdbSpawn(e.to_string())),
                Ok(Ok(out)) => out,
            }
        };

        Ok(CommandOutput {
            status: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
            stderr: String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        })
    }

    /// Like [`Self::exec`] but fails on a non-zero exit status.
    pub async fn exec_ok(
        &self,
        args: &[&str],
        timeout: Duration,
        cancel: &CancelToken,
    ) -> CoreResult<CommandOutput> {
        let out = self.exec(args, timeout, cancel).await?;
        if out.success() {
            Ok(out)
        } else {
            Err(CoreError::AdbFailed {
                code: out.status,
                stdout: out.stdout,
                stderr: out.stderr,
            })
        }
    }

    /// Run a command scoped to one device (`adb -s <serial> ...`).
    pub async fn exec_device(
        &self,
        serial: &str,
        args: &[&str],
        timeout: Duration,
        cancel: &CancelToken,
    ) -> CoreResult<CommandOutput> {
        let mut full: Vec<&str> = vec!["-s", serial];
        full.extend_from_slice(args);
        self.exec(&full, timeout, cancel).await
    }

    pub async fn exec_device_ok(
        &self,
        serial: &str,
        args: &[&str],
        timeout: Duration,
        cancel: &CancelToken,
    ) -> CoreResult<CommandOutput> {
        let mut full: Vec<&str> = vec!["-s", serial];
        full.extend_from_slice(args);
        self.exec_ok(&full, timeout, cancel).await
    }

    /// `adb -s <serial> shell <args...>`, returning stdout on success.
    pub async fn shell(
        &self,
        serial: &str,
        args: &[&str],
        cancel: &CancelToken,
    ) -> CoreResult<String> {
        // ADB joins arguments into source for the remote POSIX shell. Host
        // argv alone does not preserve literal argument boundaries there.
        if args.is_empty() || args.iter().any(|arg| arg.contains('\0')) {
            return Err(CoreError::other("Invalid literal shell arguments."));
        }
        let source = args
            .iter()
            .map(|arg| format!("'{}'", arg.replace('\'', "'\\''")))
            .collect::<Vec<_>>()
            .join(" ");
        let out = self
            .exec_device_ok(serial, &["shell", &source], DEFAULT_TIMEOUT, cancel)
            .await?;
        Ok(out.stdout)
    }

    /// Stream stdout+stderr line by line; used by installs and pushes so the
    /// UI can show live progress instead of freezing until completion.
    pub async fn exec_streaming<F>(
        &self,
        args: &[&str],
        timeout: Duration,
        cancel: &CancelToken,
        mut on_line: F,
    ) -> CoreResult<CommandOutput>
    where
        F: FnMut(&str) + Send,
    {
        if cancel.is_cancelled() {
            return Err(CoreError::Cancelled);
        }

        let mut child = self.command(args).spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => CoreError::AdbNotFound,
            _ => CoreError::AdbSpawn(e.to_string()),
        })?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // stdout and stderr MUST be drained concurrently. Reading one stream to
        // completion before touching the other deadlocks as soon as the child
        // fills the other pipe's OS buffer — `adb install` does exactly that on
        // large APKs, so a sequential pump hangs until the timeout fires.
        // Each stream gets its own task; `on_line` stays on this task because the
        // callback is only `Send`, not `'static`.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<std::io::Result<(Stream, String)>>(256);

        // JoinSet aborts its children when this future itself is dropped.
        let mut readers = tokio::task::JoinSet::new();
        for (stream, pipe) in [
            stdout.map(|s| (Stream::Stdout, Box::new(s) as BoxedPipe)),
            stderr.map(|s| (Stream::Stderr, Box::new(s) as BoxedPipe)),
        ]
        .into_iter()
        .flatten()
        {
            let tx = tx.clone();
            readers.spawn(async move {
                let mut reader = BufReader::new(pipe);
                let mut bytes = Vec::new();
                loop {
                    bytes.clear();
                    match reader.read_until(b'\n', &mut bytes).await {
                        Ok(0) => break,
                        Ok(_) => {
                            if bytes.last() == Some(&b'\n') {
                                bytes.pop();
                            }
                            if bytes.last() == Some(&b'\r') {
                                bytes.pop();
                            }
                            let line = String::from_utf8_lossy(&bytes).into_owned();
                            if tx.send(Ok((stream, line))).await.is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            let _ = tx.send(Err(error)).await;
                            break;
                        }
                    }
                }
            });
        }
        // Drop the original sender so `recv` ends once both readers are done.
        drop(tx);

        let pump = async {
            let mut collected_out = String::new();
            let mut collected_err = String::new();

            while let Some(message) = rx.recv().await {
                let (stream, line) = message.map_err(|e| CoreError::AdbSpawn(e.to_string()))?;
                on_line(&line);
                let sink = match stream {
                    Stream::Stdout => &mut collected_out,
                    Stream::Stderr => &mut collected_err,
                };
                sink.push_str(&line);
                sink.push('\n');
            }

            let status = child
                .wait()
                .await
                .map_err(|e| CoreError::AdbSpawn(e.to_string()))?;
            Ok::<_, CoreError>((status, collected_out, collected_err))
        };
        tokio::pin!(pump);

        let outcome = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(CoreError::Cancelled),
            res = tokio::time::timeout(timeout, &mut pump) => match res {
                Err(_) => Err(CoreError::Timeout(timeout)),
                Ok(result) => result,
            }
        };

        // Await cleanup on normal error paths. Drop also aborts on an external
        // timeout or callback panic; never kill an unrelated shared ADB daemon.
        if outcome.is_err() {
            readers.shutdown().await;
        } else {
            while let Some(result) = readers.join_next().await {
                result.map_err(|e| CoreError::AdbSpawn(e.to_string()))?;
            }
        }
        let (status, collected_out, collected_err) = outcome?;

        Ok(CommandOutput {
            status: status.code(),
            stdout: collected_out,
            stderr: collected_err,
        })
    }

    // ---- high level helpers -------------------------------------------------

    pub async fn version(&self, cancel: &CancelToken) -> CoreResult<String> {
        let out = self.exec_ok(&["version"], DEFAULT_TIMEOUT, cancel).await?;
        Ok(out
            .stdout
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string())
    }

    pub async fn start_server(&self, cancel: &CancelToken) -> CoreResult<()> {
        self.exec_ok(&["start-server"], DEFAULT_TIMEOUT, cancel)
            .await?;
        Ok(())
    }

    pub async fn list_devices(&self, cancel: &CancelToken) -> CoreResult<Vec<AdbDeviceEntry>> {
        let out = self
            .exec_ok(&["devices", "-l"], DEFAULT_TIMEOUT, cancel)
            .await?;
        Ok(parse::parse_devices(&out.stdout))
    }

    pub async fn getprops(
        &self,
        serial: &str,
        cancel: &CancelToken,
    ) -> CoreResult<HashMap<String, String>> {
        let raw = self.shell(serial, &["getprop"], cancel).await?;
        Ok(parse::parse_getprop(&raw))
    }

    pub async fn connect(&self, addr: &str, cancel: &CancelToken) -> CoreResult<String> {
        let out = self
            .exec_ok(&["connect", addr], Duration::from_secs(25), cancel)
            .await?;
        let text = out.combined();
        let lower = text.to_ascii_lowercase();
        // `adb connect` exits 0 even when it failed, so inspect the text.
        if lower.contains("unable to connect")
            || lower.contains("failed to connect")
            || lower.contains("cannot connect")
            || lower.contains("connection refused")
        {
            return Err(CoreError::Wireless(text.trim().to_string()));
        }
        Ok(text.trim().to_string())
    }

    pub async fn disconnect(&self, addr: &str, cancel: &CancelToken) -> CoreResult<()> {
        self.exec_ok(&["disconnect", addr], DEFAULT_TIMEOUT, cancel)
            .await?;
        Ok(())
    }

    /// Switch a USB-attached device into TCP/IP mode on `port`.
    pub async fn tcpip(&self, serial: &str, port: u16, cancel: &CancelToken) -> CoreResult<()> {
        let port = port.to_string();
        let out = self
            .exec_device_ok(serial, &["tcpip", &port], Duration::from_secs(25), cancel)
            .await?;
        let text = out.combined();
        if text.to_ascii_lowercase().contains("error") {
            return Err(CoreError::Wireless(text.trim().to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_client() -> AdbClient {
        AdbClient::new("Z:/vrmanager/does/not/exist/adb.exe")
    }

    #[tokio::test]
    async fn missing_binary_reports_adb_not_found() {
        let client = fake_client();
        let err = client
            .exec(&["version"], Duration::from_secs(5), &CancelToken::new())
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::AdbNotFound), "got {err:?}");
    }

    #[tokio::test]
    async fn pre_cancelled_token_short_circuits() {
        let client = fake_client();
        let cancel = CancelToken::new();
        cancel.cancel();
        let err = client
            .exec(&["version"], Duration::from_secs(5), &cancel)
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::Cancelled));
    }

    #[test]
    fn combined_output_includes_both_streams() {
        let out = CommandOutput {
            status: Some(0),
            stdout: "out".into(),
            stderr: "err".into(),
        };
        assert!(out.success());
        assert!(out.combined().contains("out") && out.combined().contains("err"));
    }
}
