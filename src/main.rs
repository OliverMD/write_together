use std::{io, thread};

use crate::app::{App, AppCommands, AppMessages};
use clap::Clap;
use crossterm::event::{Event, EventStream, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use futures::FutureExt;
use itertools::Itertools;
use std::iter::FromIterator;
use std::net::SocketAddr;
use std::str::FromStr;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use tokio_stream::StreamExt;
use tui::backend::CrosstermBackend;
use tui::layout::{Alignment, Constraint, Direction, Layout};
use tui::style::{Color, Style};
use tui::text::{Span, Text};
use tui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use tui::Terminal;

mod app;
mod sessions;

#[derive(Debug)]
enum InputAction {
    Quit,
    Key(KeyCode),
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum Element {
    Input,
    Connect,
}

#[derive(Clap)]
struct Opts {
    #[clap(short, long)]
    port: u16,
}

#[tokio::main]
pub async fn main() {
    let opts = Opts::parse();

    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();
    enable_raw_mode().unwrap();
    terminal.clear().unwrap();

    let (tx, mut rx) = mpsc::channel(32);
    let (ui_tx, mut ui_rx) = mpsc::channel::<AppCommands>(32);
    let (app_tx, mut app_rx) = mpsc::channel::<AppMessages>(32);
    let mut reader = EventStream::new();

    // Local IO task
    let input_task = tokio::task::spawn(async move {
        loop {
            let mut event = reader.next().await;
            if let Some(Ok(event)) = event {
                if event == Event::Key(KeyCode::Esc.into()) {
                    tx.send(InputAction::Quit).await;
                }

                if let Event::Key(event) = event {
                    tx.send(InputAction::Key(event.code)).await;
                }
            }
        }
    });

    let mut app = App::new(ui_rx, app_tx, opts.port);

    let app_task = tokio::task::spawn(async move { app.run().await });

    let mut content_buffer = Vec::new();
    let mut input_buffer = Vec::new();
    let mut address_buffer = Vec::new();
    let mut log_buffer = Vec::new();
    let mut selected_element = Element::Connect;

    let mut can_input = false;

    loop {
        terminal
            .draw(|f| {
                let size = f.size();

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(2)
                    .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
                    .split(size);

                let para = Paragraph::new(content_buffer.join(" "))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                            .title("Content"),
                    )
                    .wrap(Wrap { trim: false });

                f.render_widget(para, chunks[0]);

                let bottom_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
                    .split(chunks[1]);

                let input_para = Paragraph::new(String::from_iter(&input_buffer))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                            .style(get_style(Element::Input, selected_element))
                            .title("Input"),
                    )
                    .wrap(Wrap { trim: false });
                f.render_widget(input_para, bottom_chunks[0]);

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(3), Constraint::Min(1)])
                    .split(bottom_chunks[1]);

                let address_input = Paragraph::new(String::from_iter(&address_buffer))
                    .block(
                        Block::default()
                            .title("Connect")
                            .borders(Borders::ALL)
                            .style(get_style(Element::Connect, selected_element))
                            .border_type(BorderType::Plain),
                    )
                    .alignment(Alignment::Center);

                f.render_widget(address_input, chunks[0]);
                let log_block =
                    Paragraph::new(log_buffer.join("\n")).block(Block::default().title("Log"));

                f.render_widget(log_block, chunks[1]);
            })
            .unwrap();

        tokio::select! {
            Some(message) = app_rx.recv() => {
                match message {
                    AppMessages::Log(msg) => log_buffer.push(msg),
                    AppMessages::MoreInput(input) => content_buffer.push(input),
                    AppMessages::OurTurn => can_input = true,
                    AppMessages::NotOurTurn => can_input = false,
                }
            }
            Some(action) = rx.recv() => {
                match action {
                    InputAction::Quit => break,
                    InputAction::Key(keycode) => match keycode {
                        KeyCode::Backspace => {
                            match selected_element {
                                Element::Input => input_buffer.pop(),
                                Element::Connect => address_buffer.pop(),
                            };
                        }
                        KeyCode::Enter => {
                            if selected_element == Element::Connect {
                                let address =
                                    SocketAddr::from_str(String::from_iter(&address_buffer).as_str());

                                if let Ok(address) = address {
                                    ui_tx.send(AppCommands::Connect(address)).await;
                                }
                            }
                        }
                        KeyCode::Left => {
                            if selected_element == Element::Connect {
                                selected_element = Element::Input
                            }
                        }
                        KeyCode::Right => {
                            if selected_element == Element::Input {
                                selected_element = Element::Connect;
                            }
                        }
                        KeyCode::Char(c) => match selected_element {
                            Element::Input => {
                                if can_input {
                                    input_buffer.push(c);
                                    if c == '.' {
                                        ui_tx.send(AppCommands::Input(String::from_iter(&input_buffer))).await;
                                        input_buffer.clear();
                                    }
                                }
                            },
                            Element::Connect => address_buffer.push(c),
                        },
                        _ => {}
                    },
                }
            }
        }
    }

    disable_raw_mode().unwrap();
    terminal.clear().unwrap();
}

fn get_style(this_element: Element, selected_element: Element) -> Style {
    if selected_element == this_element {
        Style::default().fg(Color::Green)
    } else {
        Style::default()
    }
}
