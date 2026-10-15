use std::{
    io::{BufRead, Write, stdout},
    os::{
        linux::net::SocketAddrExt as _,
        unix::net::{SocketAddr, UnixStream},
    },
};

use clap::Args;
use color_eyre::Result;
use crossterm::event::{EnableFocusChange, KeyCode, KeyEvent, KeyModifiers};
use futures_concurrency::future::Race;
use ratatui::{
    DefaultTerminal, crossterm,
    layout::{Constraint, Layout},
    style::Stylize,
    text::Line,
    widgets::{Block, Paragraph, Widget, Wrap},
};
use smol::{
    Async,
    channel::*,
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    stream::StreamExt,
};
use zeroize::{Zeroize, Zeroizing};

use crate::messages::{AuthResponses, StartAuth, UserResponse};

#[derive(Args)]
pub struct TuiArgs {
    socket: String,
    token: String,
    cookie: String,
}

pub enum Event {
    AuthResult(AuthResponses),
    Char(char),
    Del,
    Submit,
    Quit,
}

pub enum Request {
    Submit {
        user: String,
        password: Zeroizing<String>,
    },
    Cancel,
}

pub struct App {
    events: Receiver<Event>,
    requests: Sender<Request>,
    password: Zeroizing<String>,
    failed: bool,
    prompt: String,
    message: String,
    users: Vec<String>,
}

impl App {
    pub fn new(
        events: Receiver<Event>,
        requests: Sender<Request>,
        pipe_reader: &UnixStream,
    ) -> Result<Self> {
        let mut buf = String::new();
        let mut reader = std::io::BufReader::new(pipe_reader);
        reader.read_line(&mut buf)?;
        let request = serde_json::from_str::<StartAuth>(&buf)?;

        Ok(App {
            events,
            requests,
            password: String::new().into(),
            failed: false,
            prompt: request.prompt.to_string(),
            message: request.message.to_string(),
            users: request.users,
        })
    }
    pub async fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        loop {
            terminal.draw(|frame| {
                frame.render_widget(&self, frame.area());
            })?;
            self.failed = false;
            match self.events.recv().await? {
                Event::AuthResult(auth_result) => match auth_result {
                    AuthResponses::Success => return Ok(()),
                    AuthResponses::Cancel => return Ok(()),
                    AuthResponses::Retry { message } => {
                        self.failed = true;
                        self.message = message;
                        self.password.clear();
                    }
                },
                Event::Char(char) => {
                    self.password.push(char);
                }
                Event::Del => {
                    self.password.pop();
                }
                Event::Submit => {
                    self.requests
                        .send(Request::Submit {
                            user: self.users[0].clone(),
                            password: self.password.clone(),
                        })
                        .await?;
                }
                Event::Quit => {
                    return Ok(());
                }
            }
        }
    }
}
impl Widget for &App {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let [header, message, users, password, confirm] = area.layout(&Layout::vertical([
            Constraint::Length(2),
            Constraint::Percentage(100),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
        ]));

        Line::raw("Authentication Required")
            .centered()
            .bold()
            .render(header, buf);

        Paragraph::new(self.message.as_str())
            .wrap(Wrap { trim: false })
            .render(message, buf);

        let passwd = if self.password.is_empty() {
            Line::raw("Password").gray()
        } else {
            Line::raw(self.password.chars().map(|_| '*').collect::<String>())
        };
        let mut block = Block::bordered().title("Password");
        if self.failed {
            block = block.red();
        }
        Paragraph::new(passwd).block(block).render(password, buf);

        let [cancel, confirm] = confirm.layout(&Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Fill(1),
        ]));
        Paragraph::new("Cancel")
            .block(Block::bordered())
            .centered()
            .red()
            .render(cancel, buf);
        Paragraph::new("Confirm")
            .block(Block::bordered())
            .centered()
            .green()
            .render(confirm, buf);
    }
}
async fn crossterm_events(events: Sender<Event>) -> Result<()> {
    let mut event_stream = crossterm::event::EventStream::new();

    while let Some(event) = event_stream.next().await {
        match event? {
            crossterm::event::Event::FocusLost => {
                events.send(Event::Quit).await?;
            }
            crossterm::event::Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                ..
            }) => {
                events.send(Event::Quit).await?;
            }
            crossterm::event::Event::Key(KeyEvent {
                code: KeyCode::Char(char),
                ..
            }) => {
                events.send(Event::Char(char)).await?;
            }
            crossterm::event::Event::Key(KeyEvent {
                code: KeyCode::Enter,
                ..
            }) => {
                events.send(Event::Submit).await?;
            }
            crossterm::event::Event::Key(KeyEvent {
                code: KeyCode::Backspace,
                ..
            }) => {
                events.send(Event::Del).await?;
            }
            _ => {}
        }
    }
    Ok(())
}
async fn polkit_events(mut reader: Async<UnixStream>, events: Sender<Event>) -> Result<()> {
    let mut buf = String::new();
    let mut reader = BufReader::new(&mut reader);

    loop {
        buf.clear();
        reader.read_line(&mut buf).await?;

        if let Ok(result) = serde_json::from_str::<AuthResponses>(&buf) {
            events.send(Event::AuthResult(result)).await?;
            // events.send(Event::AuthResult(result)).await?;
        }
    }
}
async fn request_handler(
    mut writer: Async<UnixStream>,
    requests: Receiver<Request>,
    events: Sender<Event>,
    cookie: String,
) -> Result<()> {
    let mut buf = Vec::new();

    while let Ok(request) = requests.recv().await {
        match request {
            Request::Cancel => {
                serde_json::to_writer(&mut buf, &UserResponse::Cancel { cookie: &cookie })?;
                writer.write_all(&buf).await?;
                events.send(Event::Quit).await?;
            }
            Request::Submit { user, password } => {
                serde_json::to_writer(
                    &mut buf,
                    &UserResponse::Submit {
                        cookie: &cookie,
                        username: &user,
                        password: &password,
                    },
                )?;
                writer.write_all(&buf).await?;
                buf.zeroize();
            }
        }
        writer.write_all(b"\n").await?;
    }
    Ok(())
}

fn init_term() -> Result<()> {
    crossterm::execute!(stdout(), EnableFocusChange)?;
    Ok(())
}
pub fn run(args: TuiArgs) -> Result<()> {
    let mut writer = UnixStream::connect_addr(&SocketAddr::from_abstract_name(args.socket)?)?;
    let reader = writer.try_clone()?;

    writer.write_all(args.token.as_bytes())?;

    init_term()?;
    let mut terminal = ratatui::init();

    let result = smol::block_on(async {
        let (event_sender, event_receiver) = smol::channel::bounded(16);
        let (request_sender, request_receiver) = smol::channel::bounded(16);
        let app = App::new(event_receiver, request_sender, &reader)?;
        (
            crossterm_events(event_sender.clone()),
            polkit_events(Async::new(reader)?, event_sender.clone()),
            request_handler(
                Async::new(writer)?,
                request_receiver,
                event_sender.clone(),
                args.cookie,
            ),
            app.run(&mut terminal),
        )
            .race()
            .await
    });

    ratatui::restore();
    result
}
