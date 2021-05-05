use std::io;

use crate::{
    app::{App, AppMessages},
    error::Error,
};
use clap::Clap;
use crossterm::{
    event::{Event, EventStream, KeyCode, KeyEvent},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use std::{
    fmt::{Display, Formatter},
    iter::FromIterator,
    net::SocketAddr,
    str::FromStr,
};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame, Terminal,
};

mod app;
mod error;

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

// Events emanating from UI that need to be handled by the app
#[derive(Debug)]
pub(crate) enum UIOutput {
    Connect(SocketAddr),
    Input(String),
}

impl Display for UIOutput {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            UIOutput::Connect(_) => write!(f, "Connect"),
            UIOutput::Input(_) => write!(f, "Input"),
        }
    }
}

#[tokio::main]
pub async fn main() -> Result<(), Error> {
    let opts = Opts::parse();

    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();
    enable_raw_mode().unwrap();
    terminal.clear().unwrap();

    let (ui_tx, ui_rx) = mpsc::channel::<UIOutput>(32);
    let mut reader = EventStream::new();

    let (mut app, mut app_rx) = App::new(ui_rx, opts.port);

    tokio::task::spawn(async move {
        app.run().await?;
        Ok::<(), Error>(())
    });

    let mut view_state = ViewState::new();

    loop {
        terminal.draw(|frame| view(frame, &view_state)).unwrap();

        tokio::select! {
            Some(message) = app_rx.recv() => {
                match message {
                    AppMessages::Log(msg) => view_state.log_buffer.push(msg),
                    AppMessages::MoreInput(input) => view_state.content_buffer.push(input),
                    AppMessages::OurTurn => view_state.can_input = true,
                    AppMessages::NotOurTurn => view_state.can_input = false,
                }
            }
            Some(Ok(event)) = reader.next() => {
                match event {
                    Event::Key(KeyEvent {code: KeyCode::Esc, ..}) => break,
                    Event::Key(KeyEvent {code, ..}) => match code {
                        KeyCode::Backspace => {
                            match view_state.selected_element {
                                Element::Input => view_state.input_buffer.pop(),
                                Element::Connect => view_state.address_buffer.pop(),
                            };
                        }
                        KeyCode::Enter => {
                            if view_state.selected_element == Element::Connect {
                                let address =
                                    SocketAddr::from_str(String::from_iter(&view_state.address_buffer).as_str());

                                if let Ok(address) = address {
                                    ui_tx.send(UIOutput::Connect(address)).await?;
                                }
                            }
                        }
                        KeyCode::Left => {
                            if view_state.selected_element == Element::Connect {
                                view_state.selected_element = Element::Input
                            }
                        }
                        KeyCode::Right => {
                            if view_state.selected_element == Element::Input {
                                view_state.selected_element = Element::Connect;
                            }
                        }
                        KeyCode::Char(c) => match view_state.selected_element {
                            Element::Input => {
                                if view_state.can_input {
                                    view_state.input_buffer.push(c);
                                    if c == '.' {
                                        ui_tx.send(UIOutput::Input(String::from_iter(&view_state.input_buffer))).await?;
                                        view_state.input_buffer.clear();
                                    }
                                }
                            },
                            Element::Connect => view_state.address_buffer.push(c),
                        },
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode().unwrap();
    terminal.clear().unwrap();

    Ok(())
}

struct ViewState {
    content_buffer: Vec<String>,
    log_buffer: Vec<String>,

    input_buffer: Vec<char>,
    address_buffer: Vec<char>,
    selected_element: Element,

    can_input: bool,
}

impl ViewState {
    fn new() -> ViewState {
        ViewState {
            content_buffer: Vec::new(),
            input_buffer: Vec::new(),
            address_buffer: Vec::new(),
            log_buffer: Vec::new(),
            selected_element: Element::Input,
            can_input: false,
        }
    }
}

fn view<B: Backend>(frame: &mut Frame<B>, state: &ViewState) {
    let size = frame.size();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
        .split(size);

    let para = Paragraph::new(state.content_buffer.join(" "))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title("Content"),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(para, chunks[0]);

    let bottom_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
        .split(chunks[1]);

    let input_para = Paragraph::new(String::from_iter(&state.input_buffer))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .style(get_style(Element::Input, state.selected_element))
                .title("Input"),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(input_para, bottom_chunks[0]);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(bottom_chunks[1]);

    let address_input = Paragraph::new(String::from_iter(&state.address_buffer))
        .block(
            Block::default()
                .title("Connect")
                .borders(Borders::ALL)
                .style(get_style(Element::Connect, state.selected_element))
                .border_type(BorderType::Plain),
        )
        .alignment(Alignment::Center);

    frame.render_widget(address_input, chunks[0]);
    let log_block =
        Paragraph::new(state.log_buffer.join("\n")).block(Block::default().title("Log"));

    frame.render_widget(log_block, chunks[1])
}

fn get_style(this_element: Element, selected_element: Element) -> Style {
    if selected_element == this_element {
        Style::default().fg(Color::Green)
    } else {
        Style::default()
    }
}
