use crate::error::Error;
use crate::UIOutput;
use std::fmt::{Display, Formatter};
use std::net::{IpAddr, SocketAddr};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender};

// Messages from the app that should be handled by the UI.
#[derive(Debug)]
pub(crate) enum AppMessages {
    Log(String),
    MoreInput(String),
    OurTurn,
    NotOurTurn,
}

impl Display for AppMessages {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AppMessages::Log(_) => write!(f, "Log"),
            AppMessages::MoreInput(_) => write!(f, "MoreInput"),
            AppMessages::OurTurn => write!(f, "OurTurn"),
            AppMessages::NotOurTurn => write!(f, "NotOurTurn"),
        }
    }
}

#[derive(Debug)]
enum State {
    Waiting,
    Connected(TcpStream),
}

#[derive(Debug)]
pub(crate) struct App {
    ui_rx: Receiver<UIOutput>,
    app_tx: Sender<AppMessages>,
    state: State,
    listen_port: u16,
}

impl App {
    pub(crate) fn new(ui_rx: Receiver<UIOutput>, listen_port: u16) -> (App, Receiver<AppMessages>) {
        let (app_tx, app_rx) = mpsc::channel::<AppMessages>(32);

        return (
            App {
                ui_rx,
                app_tx,
                state: State::Waiting,
                listen_port,
            },
            app_rx,
        );
    }

    pub(crate) async fn run(&mut self) -> Result<(), Error> {
        let listener = TcpListener::bind(SocketAddr::new(
            IpAddr::from([127, 0, 0, 1]),
            self.listen_port,
        ))
        .await?;

        loop {
            match &mut self.state {
                State::Waiting => {
                    tokio::select! {
                        Ok((socket, remote)) = listener.accept() => {
                            self.app_tx.send(AppMessages::Log(format!("Connected to {:?}", remote))).await?;
                            self.state = State::Connected(socket);
                            self.app_tx.send(AppMessages::OurTurn).await?;
                            // Someone trying to connect after this point could time out
                        }
                        Some(cmd) = self.ui_rx.recv() => {
                            match cmd {
                                UIOutput::Connect(addr) => {
                                    self.app_tx.send(AppMessages::Log(format!("Attempting to connect to {:?}", addr))).await?;
                                    let socket = TcpStream::connect(addr).await?;
                                    self.state = State::Connected(socket);
                                    self.app_tx.send(AppMessages::Log(format!("Connected to remote {:?}", addr))).await?;
                                }
                                UIOutput::Input(_) => {
                                    self.app_tx.send(AppMessages::Log("ERROR: Unexpected input".to_string())).await?;
                                }
                            }
                        }
                        else => {
                            return Ok(());
                        }
                    }
                }
                State::Connected(stream) => {
                    let mut buf = vec![0; 1024];
                    tokio::select! {
                        Ok(result) = stream.read(&mut buf) => {
                            if result > 0 {
                                self.app_tx.send(AppMessages::Log(format!("{:?}", &buf.as_slice()[..result]))).await?;
                                self.app_tx.send(AppMessages::MoreInput(String::from_utf8(buf).unwrap())).await?;
                                self.app_tx.send(AppMessages::OurTurn).await?;
                            } else {
                                // Socket closed
                                self.state = State::Waiting;
                            }
                        }
                        Some(cmd) = self.ui_rx.recv() => {
                            match cmd {
                                UIOutput::Input(input) => {
                                    stream.write_all(input.as_bytes()).await?;
                                    self.app_tx.send(AppMessages::MoreInput(input)).await?;
                                    self.app_tx.send(AppMessages::NotOurTurn).await?;
                                }
                                _ => {
                                    self.app_tx.send(AppMessages::Log("Unexpected Command".to_string())).await?;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
