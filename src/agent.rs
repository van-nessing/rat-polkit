use crate::{
    authority::{PolkitError, Result},
    handler::{HandlerArgs, spawn_tui},
    messages::{AuthResponses, StartAuth, UserResponse},
    polkit_helper::PolkitHelperConfig,
};
use std::{collections::HashMap, sync::Arc};

use futures_concurrency::future::Race;
use smol::{
    channel::{Receiver, Sender},
    io::{AsyncBufReadExt as _, AsyncWriteExt, BufReader},
    net::unix::UnixStream,
    process::Child,
    stream::StreamExt,
};
use zbus::interface;

use crate::authority::Identity;

pub struct Shared {
    handler_args: HandlerArgs,
    helper: PolkitHelperConfig,
}

pub struct Session {
    shared: Arc<Shared>,
    action_id: String,
    message: String,
    icon_name: String,
    details: HashMap<String, String>,
    cookie: String,
    identities: Vec<Identity>,
    cancel_receiver: Receiver<String>,
}

pub struct AuthenticationAgent {
    shared: Arc<Shared>,
    sender: Sender<String>,
    receiver: Receiver<String>,
}
impl AuthenticationAgent {
    pub const OBJECT_PATH: &str = "/org/ratpolkit/PolicyKit1/AuthenticationAgent";
    pub fn new(handler_args: HandlerArgs, helper: PolkitHelperConfig) -> Self {
        let (sender, receiver) = smol::channel::bounded(1);
        Self {
            shared: Arc::new(Shared {
                handler_args,
                helper,
            }),
            sender,
            receiver,
        }
    }
}

impl Session {
    pub async fn init_ui_process(&mut self) -> Result<(UnixStream, Child)> {
        let names = self
            .identities
            .iter()
            .filter(|identity| &identity.kind == "unix-user")
            .filter_map(|identity| {
                let details = &identity.details;
                details["uid"]
                    .downcast_ref::<u32>()
                    .ok()
                    .and_then(|uid| etc_passwd::Passwd::from_uid(uid).ok().flatten())
                    .and_then(|passwd| passwd.name.into_string().ok())
            })
            .collect();
        let (mut stream, child) = spawn_tui(&self.shared.handler_args, &self.cookie)
            .await
            .map_err(|_| PolkitError::Failed("Failed to spawn tui".to_string()))?;

        let init = serde_json::to_vec(&StartAuth {
            prompt: "Password",
            message: &self.message,
            users: names,
        })
        .unwrap();
        stream.write_all(&init).await?;
        stream.write_all(b"\n").await?;
        Ok((stream, child))
    }
    pub async fn run(&mut self) -> Result<()> {
        let (stream, mut child) = self.init_ui_process().await?;

        let (reader, mut child_writer) = smol::io::split(stream);
        let child_reader = BufReader::new(reader);

        let mut lines = child_reader.lines();
        let result = (
            async {
                loop {
                    let Some(Ok(line)) = lines.next().await else {
                        break Err(PolkitError::Failed("Channel closed".to_string()));
                    };
                    let response = serde_json::from_str::<UserResponse>(&line)
                        .map_err(|_| PolkitError::Failed("Failed to parse response".to_string()))?;
                    match response {
                        UserResponse::Submit {
                            cookie,
                            username,
                            password,
                        } => {
                            if cookie != self.cookie {
                                continue;
                            }
                            let mut helper =
                                self.shared.helper.new_helper(username, cookie).await?;
                            let event = helper.process_events(password).await?;
                            let resp = serde_json::to_vec(&event).unwrap();

                            child_writer.write_all(&resp).await?;
                            child_writer.write_all(b"\n").await?;
                            if let AuthResponses::Success = event {
                                break Ok(());
                            }
                        }
                        UserResponse::Cancel { cookie } => {
                            if cookie != self.cookie {
                                continue;
                            }
                            break Err(PolkitError::Cancelled(
                                "User cancelled the authentication.".to_string(),
                            ));
                        }
                    }
                }
            },
            async {
                loop {
                    let cookie = self
                        .cancel_receiver
                        .recv()
                        .await
                        .map_err(|_| PolkitError::Failed("no clue".to_string()))?;
                    if cookie == self.cookie {
                        break Err(PolkitError::Cancelled(
                            "User cancelled the authentication.".to_string(),
                        ));
                    }
                }
            },
        )
            .race()
            .await;
        let _ = child.kill();
        result
    }
}
#[interface(name = "org.freedesktop.PolicyKit1.AuthenticationAgent")]
impl AuthenticationAgent {
    async fn cancel_authentication(&self, cookie: String) {
        let _ = self.sender.send(cookie).await;
    }
    async fn begin_authentication(
        &self,
        action_id: String,
        message: String,
        icon_name: String,
        details: HashMap<String, String>,
        cookie: String,
        identities: Vec<Identity>,
    ) -> Result<()> {
        Session {
            shared: self.shared.clone(),
            action_id,
            message,
            icon_name,
            details,
            cookie,
            identities,
            cancel_receiver: self.receiver.clone(),
        }
        .run()
        .await
    }
}
