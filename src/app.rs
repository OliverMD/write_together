use crate::sessions::SessionInstance;
use futures::future::OptionFuture;
use futures::FutureExt;
use std::net::{IpAddr, SocketAddr};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::sync::mpsc::{Receiver, Sender};

pub(crate) enum AppCommands {
    Connect(SocketAddr),
    Input(String),
}

pub(crate) enum AppMessages {
    Log(String),
    MoreInput(String),
    OurTurn,
    NotOurTurn,
}

#[derive(Debug)]
enum State {
    Waiting,
    Connected((TcpStream, SessionInstance)),
}

#[derive(Debug)]
pub(crate) struct App {
    ui_rx: Receiver<AppCommands>,
    app_tx: Sender<AppMessages>,
    state: State,
    listen_port: u16,
}

impl App {
    pub(crate) fn new(
        mut ui_rx: Receiver<AppCommands>,
        app_tx: Sender<AppMessages>,
        listen_port: u16,
    ) -> App {
        return App {
            ui_rx,
            app_tx,
            state: State::Waiting,
            listen_port,
        };
    }

    pub(crate) async fn run(&mut self) -> io::Result<()> {
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
                            self.app_tx.send(AppMessages::Log(format!("Connected to {:?}", remote))).await;
                            self.state = State::Connected((socket, SessionInstance::new(0)));
                            self.app_tx.send(AppMessages::OurTurn).await;
                            // Someone trying to connect after this point could time out
                        }
                        Some(cmd) = self.ui_rx.recv() => {
                            match cmd {
                                AppCommands::Connect(addr) => {
                                    self.app_tx.send(AppMessages::Log(format!("Attempting to connect to {:?}", addr))).await;
                                    let mut socket = TcpStream::connect(addr).await?;
                                    self.state = State::Connected((socket, SessionInstance::new(1)));
                                    self.app_tx.send(AppMessages::Log(format!("Connected to remote {:?}", addr))).await;
                                }
                                AppCommands::Input(_) => {
                                    self.app_tx.send(AppMessages::Log("ERROR: Unexpected input".to_string())).await;
                                }
                            }
                        }
                        else => {
                            return Ok(());
                        }
                    }
                }
                State::Connected((stream, session)) => {
                    let mut buf = vec![0; 1024];
                    tokio::select! {
                        Ok(result) = stream.read(&mut buf) => {
                            if result > 0 {
                                self.app_tx.send(AppMessages::Log(format!("{:?}", &buf.as_slice()[..result]))).await;
                                self.app_tx.send(AppMessages::MoreInput(String::from_utf8(buf).unwrap())).await;
                                self.app_tx.send(AppMessages::OurTurn).await;
                            } else {
                                // Socket closed
                                self.state = State::Waiting;
                            }
                        }
                        Some(cmd) = self.ui_rx.recv() => {
                            match cmd {
                                AppCommands::Input(input) => {
                                    stream.write_all(input.as_bytes()).await?;
                                    self.app_tx.send(AppMessages::MoreInput(input)).await;
                                    self.app_tx.send(AppMessages::NotOurTurn).await;
                                }
                                _ => {
                                    self.app_tx.send(AppMessages::Log("Unexpected Command".to_string())).await;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
