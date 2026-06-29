use std::{path::PathBuf, pin::Pin, process::Stdio};

use pin_project::pin_project;
use smol::{
    io::{
        AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt as _, BufReader,
        ReadHalf, WriteHalf,
    },
    net::unix::UnixStream,
    process::{Child, ChildStdin, ChildStdout},
};

use crate::{authority::PolkitError, messages::AuthResponses};

pub struct PolkitHelperConfig {
    socket: PathBuf,
    binary: PathBuf,
}
impl Default for PolkitHelperConfig {
    fn default() -> Self {
        Self {
            socket: PathBuf::from("/run/polkit/agent-helper.socket"),
            binary: Default::default(),
        }
    }
}

#[pin_project(project = PolkitHelperProj)]
pub enum PolkitHelper {
    Socket {
        #[pin]
        read: BufReader<ReadHalf<UnixStream>>,
        #[pin]
        write: WriteHalf<UnixStream>,
    },
    Binary {
        child: Child,
        #[pin]
        read: BufReader<ChildStdout>,
        #[pin]
        write: ChildStdin,
    },
}
impl PolkitHelper {
    pub async fn process_events(&mut self, pw: &str) -> Result<AuthResponses, PolkitError> {
        let mut line = String::new();
        let mut prev_info = None;
        loop {
            line.clear();
            self.read_line(&mut line).await?;

            if let Some(sliced) = line.strip_prefix("PAM_PROMPT_ECHO_OFF") {
                if sliced.trim() == "Password:" {
                    self.write_all(pw.as_bytes()).await?;
                    self.write_all(b"\n").await?;
                }
            } else if let Some(info) = line.strip_prefix("PAM_TEXT_INFO") {
                let msg = info.trim().to_string();

                if msg.contains("minute") && msg.contains("unlock") {
                    prev_info = Some(msg.clone());
                    return Ok(AuthResponses::Retry { message: msg });
                }
            } else if line.starts_with("FAILURE") {
                let retry_msg = prev_info
                    .unwrap_or_else(|| String::from("Authentication failed. Please try again."));
                return Ok(AuthResponses::Retry { message: retry_msg });
            } else if line.starts_with("SUCCESS") {
                return Ok(AuthResponses::Success);
            }
        }
    }
}
impl PolkitHelperConfig {
    pub async fn new_helper(&self, user: &str, cookie: &str) -> Result<PolkitHelper, PolkitError> {
        if let Some(helper) = self.new_socket_helper(user, cookie).await? {
            Ok(helper)
        } else {
            self.new_binary_helper(user, cookie).await
        }
    }
    async fn new_socket_helper(
        &self,
        user: &str,
        cookie: &str,
    ) -> Result<Option<PolkitHelper>, PolkitError> {
        let Ok(stream) = UnixStream::connect(&self.socket).await else {
            return Ok(None);
        };
        let (read, mut write) = smol::io::split(stream);

        write.write_all(user.as_bytes()).await?;
        write.write_all(b"\n").await?;
        write.write_all(cookie.as_bytes()).await?;
        write.write_all(b"\n").await?;
        Ok(Some(PolkitHelper::Socket {
            read: BufReader::new(read),
            write,
        }))
    }
    async fn new_binary_helper(
        &self,
        user: &str,
        cookie: &str,
    ) -> Result<PolkitHelper, PolkitError> {
        let mut child = smol::process::Command::new(&self.binary)
            .arg(user)
            .env("LC_ALL", "C")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| {
                PolkitError::Failed("Failed to the spawn polkit authentication helper.".to_string())
            })?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or(PolkitError::Failed("Child did not have stdin.".to_string()))?;
        let stdout = child.stdout.take().ok_or(PolkitError::Failed(
            "Child did not have stdout.".to_string(),
        ))?;

        stdin.write_all(cookie.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        Ok(PolkitHelper::Binary {
            child,
            read: BufReader::new(stdout),
            write: stdin,
        })
    }
}

impl AsyncBufRead for PolkitHelper {
    fn poll_fill_buf(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<&[u8]>> {
        match self.project() {
            PolkitHelperProj::Socket { read, .. } => read.poll_fill_buf(cx),
            PolkitHelperProj::Binary { read, .. } => read.poll_fill_buf(cx),
        }
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        match self.project() {
            PolkitHelperProj::Socket { read, .. } => read.consume(amt),
            PolkitHelperProj::Binary { read, .. } => read.consume(amt),
        }
    }
}
impl AsyncRead for PolkitHelper {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.project() {
            PolkitHelperProj::Socket { read, .. } => read.poll_read(cx, buf),
            PolkitHelperProj::Binary { read, .. } => read.poll_read(cx, buf),
        }
    }
}
impl AsyncWrite for PolkitHelper {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.project() {
            PolkitHelperProj::Socket { write, .. } => write.poll_write(cx, buf),
            PolkitHelperProj::Binary { write, .. } => write.poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.project() {
            PolkitHelperProj::Socket { write, .. } => write.poll_flush(cx),
            PolkitHelperProj::Binary { write, .. } => write.poll_flush(cx),
        }
    }

    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.project() {
            PolkitHelperProj::Socket { write, .. } => write.poll_close(cx),
            PolkitHelperProj::Binary { write, .. } => write.poll_close(cx),
        }
    }
}
