use crate::{error::Error, ui_actor::UIHandle};
use std::{
    fmt::{Display, Formatter},
    net::{IpAddr, SocketAddr},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{
        mpsc,
        mpsc::{Receiver, Sender},
    },
};

#[derive(Debug)]
pub(crate) enum AppInput {
    Connect(SocketAddr),
    Input(String),
}

impl Display for AppInput {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AppInput::Connect(_) => write!(f, "Connect"),
            AppInput::Input(_) => write!(f, "Input"),
        }
    }
}

#[derive(Debug)]
enum State {
    Waiting,
    Connected(TcpStream),
}

#[derive(Debug)]
struct App {
    ui_handle: UIHandle,
    state: State,
    listen_port: u16,
}

impl App {
    fn new(ui_handle: UIHandle, listen_port: u16) -> Self {
        Self {
            ui_handle,
            state: State::Waiting,
            listen_port,
        }
    }

    async fn handle_message(&mut self, msg: AppInput) -> Result<(), Error> {
        match msg {
            AppInput::Connect(address) => match self.state {
                State::Waiting => {
                    self.ui_handle
                        .log(format!("Attempting to connect to {:?}", address))
                        .await?;
                    let socket = TcpStream::connect(address).await?;
                    self.state = State::Connected(socket);
                    self.ui_handle.connected(true).await?;
                    self.ui_handle
                        .log(format!("Connected to remote {:?}", address))
                        .await?;
                }
                State::Connected(_) => {}
            },
            AppInput::Input(input) => match &mut self.state {
                State::Waiting => {
                    self.ui_handle
                        .log("ERROR: Unexpected input".to_string())
                        .await?;
                }
                State::Connected(stream) => {
                    stream.write_all(input.as_bytes()).await?;
                }
            },
        }
        Ok(())
    }

    async fn process_remote(&mut self) -> Result<(), Error> {
        if let State::Connected(stream) = &mut self.state {
            let mut buf = vec![0; 1024];
            let result = stream.read(&mut buf).await?;
            if result > 0 {
                self.ui_handle
                    .log(format!("{:?}", &buf.as_slice()[..result]))
                    .await?;
                self.ui_handle
                    .turn_received(String::from_utf8(buf).unwrap())
                    .await?;
            } else {
                self.state = State::Waiting;
                self.ui_handle.disconnected().await?;
            }
        }
        Ok(())
    }

    async fn accept(&mut self, mut stream: TcpStream) -> Result<(), Error> {
        if matches!(self.state, State::Waiting) {
            self.state = State::Connected(stream);
            self.ui_handle.connected(false).await?;
        } else {
            stream.shutdown().await?;
        }
        Ok(())
    }
}

async fn run_app(mut app: App, mut receiver: Receiver<AppInput>) -> Result<(), Error> {
    let listener = TcpListener::bind(SocketAddr::new(
        IpAddr::from([127, 0, 0, 1]),
        app.listen_port,
    ))
    .await?;

    app.ui_handle
        .log(String::from("Bound to localhost"))
        .await?;

    loop {
        tokio::select! {
            Ok((socket, _addr)) = listener.accept() => {
                app.accept(socket).await?;
            }
            msg = receiver.recv() => {
                if let Some(msg) = msg {
                    app.handle_message(msg).await?;
                } else {
                    // Lost connection to the ui actor so we should die
                    break Ok(());
                }
            }
            Ok(_) = app.process_remote() => {}
            else => break Ok(()),
        }
    }
}

pub struct AppHandle {
    sender: Sender<AppInput>,
}

impl AppHandle {
    pub fn new(listen_port: u16, ui_handle: UIHandle) -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let app = App::new(ui_handle, listen_port);
        tokio::spawn(run_app(app, receiver));
        Self { sender }
    }

    pub async fn send_sentence(&self, sentence: String) -> Result<(), Error> {
        self.sender.send(AppInput::Input(sentence)).await?;
        Ok(())
    }

    pub async fn connect(&self, address: SocketAddr) -> Result<(), Error> {
        self.sender.send(AppInput::Connect(address)).await?;
        Ok(())
    }
}
