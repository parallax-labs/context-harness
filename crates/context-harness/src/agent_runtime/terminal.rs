//! A cancellable terminal-only approval reader. Piped stdin never grants consent.
use super::policy::{ApprovalHandler, ApprovalRequest};
use async_trait::async_trait;

pub(super) struct TerminalApprovals;
#[async_trait]
impl ApprovalHandler for TerminalApprovals {
    async fn approve(&self, request: &ApprovalRequest) -> bool {
        read_approval(request).await.unwrap_or(false)
    }
}

#[cfg(unix)]
async fn read_approval(request: &ApprovalRequest) -> std::io::Result<bool> {
    use std::{fs::OpenOptions, io::IsTerminal, os::unix::fs::OpenOptionsExt};

    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        return Ok(false);
    }
    let tty = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open("/dev/tty")?;
    read_from_tty(tty, request).await
}

#[cfg(unix)]
async fn read_from_tty(tty: std::fs::File, request: &ApprovalRequest) -> std::io::Result<bool> {
    use std::{
        io::{Read, Write},
        os::fd::AsRawFd,
    };
    use tokio::io::unix::AsyncFd;
    // Discard queued answers before exposing a new invocation. Consent must be
    // entered after this prompt, never reused from a previous pasted line.
    if unsafe { libc::tcflush(tty.as_raw_fd(), libc::TCIFLUSH) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let tty = AsyncFd::new(tty)?;
    // JSON escapes terminal control characters in untrusted model arguments.
    let details = serde_json::json!({"run":request.run_id,"call":request.call_id,
        "tool":request.tool,"capabilities":request.capabilities,"arguments":request.arguments});
    let details: String = serde_json::to_string_pretty(&details)?
        .chars()
        .map(|c| {
            if c == '\n' || (c.is_ascii() && !c.is_control()) {
                c.to_string()
            } else {
                c.escape_default().to_string()
            }
        })
        .collect();
    // Never ask a user to approve incomplete/truncated invocation details.
    if details.len() > 32 * 1024 {
        return Ok(false);
    }
    let warning = if request.tool == "process.exec"
        || request.tool.starts_with("runtime.mcp.start.")
        || request.tool.starts_with("mcp.")
    {
        "\nThis process is not sandboxed: it can access files, credentials, network, and child processes with your user permissions."
    } else {
        ""
    };
    eprintln!("\nAgent requests approval:\n{details}{warning}\nAllow this invocation once? Type yes to approve; anything else denies:");
    std::io::stderr().flush()?;
    let mut answer = Vec::new();
    loop {
        let mut ready = tty.readable().await?;
        let mut buf = [0u8; 128];
        let result = ready.try_io(|inner| {
            let mut file = inner.get_ref();
            file.read(&mut buf)
        });
        match result {
            Ok(Ok(0)) => return Ok(false),
            Ok(Ok(n)) => {
                answer.extend_from_slice(&buf[..n]);
                if answer.len() > 128 {
                    return Ok(false);
                }
                if answer.contains(&b'\n') {
                    return Ok(answer == b"yes\n" || answer == b"yes\r\n");
                }
            }
            Ok(Err(error)) => return Err(error),
            Err(_) => continue,
        }
    }
}
#[cfg(not(unix))]
async fn read_approval(_: &ApprovalRequest) -> std::io::Result<bool> {
    // No cancellable native terminal adapter yet; fail closed on these hosts.
    Ok(false)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{fs::File, io::Write, os::fd::FromRawFd, time::Duration};
    #[tokio::test]
    async fn queued_answers_do_not_approve_a_new_invocation() {
        let (mut master, mut slave) = (-1, -1);
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            0
        );
        let mut master = unsafe { File::from_raw_fd(master) };
        let tty = unsafe { File::from_raw_fd(slave) };
        assert_ne!(
            unsafe { libc::fcntl(slave, libc::F_SETFL, libc::O_NONBLOCK) },
            -1
        );
        master.write_all(b"yes\nyes\n").unwrap();
        // Give the line discipline time to queue the pasted answers.
        tokio::time::sleep(Duration::from_millis(20)).await;
        let request = ApprovalRequest {
            run_id: "run".into(),
            call_id: "call".into(),
            tool: "workspace.patch".into(),
            capabilities: vec![],
            arguments: serde_json::json!({}),
        };
        let future = read_from_tty(tty, &request);
        tokio::pin!(future);
        assert!(tokio::time::timeout(Duration::from_millis(30), &mut future)
            .await
            .is_err());
        master.write_all(b"yes\n").unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(1), future)
            .await
            .unwrap()
            .unwrap());
    }
}
