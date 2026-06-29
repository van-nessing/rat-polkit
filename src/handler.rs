use std::{
    collections::HashMap,
    env,
    os::{linux::net::SocketAddrExt as _, unix::net::SocketAddr},
    process::Stdio,
};

use clap::Args;
use color_eyre::{
    Result,
    eyre::{Context as _, eyre},
};
use smol::{
    io::AsyncReadExt,
    net::unix::{UnixListener, UnixStream},
    process::Child,
    stream::StreamExt,
};
use zbus::{conn, zvariant::Value};

use crate::{
    agent::AuthenticationAgent,
    authority::{AuthorityProxy, Subject},
    polkit_helper::PolkitHelperConfig,
};
#[derive(Args)]
pub struct HandlerArgs {
    #[arg(long)]
    pub terminal_bin: String,
    #[arg(long, value_parser, value_delimiter = ' ')]
    pub terminal_args: Vec<String>,
    #[arg(long)]
    pub bin: String,
}
pub async fn spawn_tui(args: &HandlerArgs, cookie: &str) -> Result<(UnixStream, Child)> {
    let mut name = [0u8; 48];
    getrandom::fill(&mut name)?;
    let name = hex::encode(name);

    let mut token_bytes = [0u8; 64];
    getrandom::fill(&mut token_bytes)?;
    let token = hex::encode(token_bytes);

    let addr = SocketAddr::from_abstract_name(&name)?;
    let listener = std::os::unix::net::UnixListener::bind_addr(&addr)?;
    let listener = UnixListener::try_from(listener)?;

    let command = smol::process::Command::new(&args.terminal_bin)
        .args(&args.terminal_args)
        .arg(&args.bin)
        .arg("tui")
        .arg(name)
        .arg(&token)
        .arg(cookie)
        .stdout(Stdio::null())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;

    let (mut stream, _) = listener.accept().await?;

    let mut token_raw = token.into_bytes();
    stream.read_exact(&mut token_raw).await?;
    let decoded = hex::decode(token_raw)?;

    if decoded == token_bytes {
        Ok((stream, command))
    } else {
        Err(eyre!("token did not match, aborting"))
    }
}
pub fn run(args: HandlerArgs) -> Result<()> {
    smol::block_on(async {
        let locale = env::var("LANG").unwrap_or_else(|_| String::from("en_US.utf8"));
        let subject_kind = "unix-session".to_string();

        let subject_details = HashMap::from([(
            "session-id".to_string(),
            Value::new(std::env::var("XDG_SESSION_ID").context(
                "Could not get XDG session id, make sure that it is set and try again.",
            )?),
        )]);
        let subject = Subject::new(subject_kind, subject_details);

        let agent = AuthenticationAgent::new(args, PolkitHelperConfig::default());
        let connection = conn::Builder::system()?
            .serve_at(AuthenticationAgent::PATH, agent)?
            .build()
            .await?;

        let proxy = AuthorityProxy::new(&connection).await?;
        proxy
            .register_authentication_agent(&subject, &locale, AuthenticationAgent::PATH)
            .await?;

        let mut owner_change_signal = proxy
            .inner()
            .receive_owner_changed()
            .await
            .context("Could not get the signal for owner change?")?;
        // smol::spawn(async move {
        // });
        while let Some(result) = owner_change_signal.next().await {
            // if we can get a UniqueName out of the stream, then polkit has restarted for some reason & we should reregister
            if result.is_some() {
                proxy
                    .register_authentication_agent(&subject, &locale, AuthenticationAgent::PATH)
                    .await
                    .context("Somehow failed to reregister ourselves?")
                    .unwrap();
            }
        }
        Ok(())
    })
}
