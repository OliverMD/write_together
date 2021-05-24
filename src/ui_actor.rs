use crate::{
    app::AppHandle,
    error::Error,
    ui_actor::AppState::{InSession, Waiting},
};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent};
use std::{
    fmt::{Display, Formatter},
    iter::FromIterator,
    net::SocketAddr,
    str::FromStr,
};
use tokio::{
    macros::support::{Future, Pin},
    sync::{mpsc, mpsc::Sender},
};
use tokio_stream::StreamExt;
use tui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame, Terminal,
};

#[derive(Debug)]
enum UIMessage {
    Log(String),
    SentenceReceived(String),
    Connected(bool),
    Disconnected,
}

impl Display for UIMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            UIMessage::Log(_) => write!(f, "Log"),
            UIMessage::SentenceReceived(_) => write!(f, "SentenceReceived"),
            UIMessage::Connected(_) => write!(f, "Connected"),
            UIMessage::Disconnected => write!(f, "Disconnected"),
        }
    }
}

enum AppState {
    InSession {
        is_our_turn: bool,
        content_log: Vec<String>,
    },
    Waiting,
}

impl AppState {
    fn content_log(&self) -> Option<String> {
        match self {
            AppState::InSession { content_log, .. } => Some(content_log.join(" ")),
            Waiting => None,
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum Element {
    Input,
    Connect,
}

struct UIActor {
    app_state: AppState,

    log_buffer: Vec<String>,

    input_buffer: Vec<char>,
    address_buffer: Vec<char>,
    selected_element: Element,

    receiver: mpsc::Receiver<UIMessage>,

    event_stream: EventStream,
    app_handle: AppHandle,
}

impl UIActor {
    fn new(
        receiver: mpsc::Receiver<UIMessage>,
        event_stream: EventStream,
        app_handle: AppHandle,
    ) -> Self {
        Self {
            app_state: Waiting,
            log_buffer: vec![],
            input_buffer: vec![],
            address_buffer: vec![],
            selected_element: Element::Connect,
            receiver,
            event_stream,
            app_handle,
        }
    }

    fn handle_message(&mut self, msg: UIMessage) {
        match msg {
            UIMessage::Log(message) => {
                self.log_buffer.push(message);
            }
            UIMessage::SentenceReceived(sentence) => {
                if let InSession {
                    is_our_turn,
                    content_log,
                } = &mut self.app_state
                {
                    *is_our_turn = true;
                    content_log.push(sentence);
                }
            }
            UIMessage::Connected(is_our_turn) => {
                self.log_buffer.push(String::from("Accepted remote connection"));
                self.app_state = InSession {
                    is_our_turn,
                    content_log: Vec::new(),
                }
            }
            UIMessage::Disconnected => self.app_state = Waiting,
        }
    }

    // Check for input that is independent of state
    fn handle_independent_event(&mut self, event: Event) -> Option<bool> {
        if let Event::Key(KeyEvent { code, .. }) = event {
            match code {
                KeyCode::Esc => Some(true),
                KeyCode::Backspace => {
                    match self.selected_element {
                        Element::Input => self.input_buffer.pop(),
                        Element::Connect => self.address_buffer.pop(),
                    };
                    Some(false)
                }
                KeyCode::Left => {
                    if self.selected_element == Element::Connect {
                        self.selected_element = Element::Input;
                    }
                    None
                }
                KeyCode::Right => {
                    if self.selected_element == Element::Input {
                        self.selected_element = Element::Connect;
                    }
                    None
                }
                _ => None,
            }
        } else {
            None
        }
    }

    async fn handle_input_event(&mut self, event: Event) -> Result<bool, Error> {
        if Some(true) == self.handle_independent_event(event) {
            return Ok(true);
        }

        match &mut self.app_state {
            InSession {
                is_our_turn,
                content_log,
            } => {
                if let Event::Key(KeyEvent {
                    code: KeyCode::Char(c),
                    ..
                }) = event
                {
                    if self.selected_element == Element::Input && *is_our_turn {
                        self.input_buffer.push(c);
                        if c == '.' {
                            self.app_handle
                                .send_sentence(String::from_iter(&self.input_buffer))
                                .await?;
                            content_log.push(String::from_iter(&self.input_buffer));
                            *is_our_turn = false;
                            self.input_buffer.clear();
                        }
                    }
                }
            }
            Waiting => {
                if let Event::Key(KeyEvent { code, .. }) = event {
                    match code {
                        KeyCode::Enter => {
                            if self.selected_element == Element::Connect {
                                let address = SocketAddr::from_str(
                                    String::from_iter(&self.address_buffer).as_str(),
                                );

                                if let Ok(address) = address {
                                    self.app_handle.connect(address).await?;
                                }
                            }
                        }
                        KeyCode::Char(c) => {
                            if self.selected_element == Element::Connect {
                                self.address_buffer.push(c)
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(false)
    }

    fn draw<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<(), Error> {
        terminal.draw(|frame| self.draw_view(frame))?;
        Ok(())
    }

    fn draw_view<B: Backend>(&self, frame: &mut Frame<B>) {
        let size = frame.size();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
            .split(size);

        let para = Paragraph::new(self.app_state.content_log().unwrap_or_default())
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

        let input_para = Paragraph::new(String::from_iter(&self.input_buffer))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .style(get_style(Element::Input, self.selected_element))
                    .title("Input"),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(input_para, bottom_chunks[0]);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(bottom_chunks[1]);

        let address_input = Paragraph::new(String::from_iter(&self.address_buffer))
            .block(
                Block::default()
                    .title("Connect")
                    .borders(Borders::ALL)
                    .style(get_style(Element::Connect, self.selected_element))
                    .border_type(BorderType::Plain),
            )
            .alignment(Alignment::Center);

        frame.render_widget(address_input, chunks[0]);
        let log_block =
            Paragraph::new(self.log_buffer.join("\n")).block(Block::default().title("Log"));

        frame.render_widget(log_block, chunks[1])
    }
}

fn get_style(this_element: Element, selected_element: Element) -> Style {
    if selected_element == this_element {
        Style::default().fg(Color::Green)
    } else {
        Style::default()
    }
}

async fn run_ui_actor<B: Backend>(
    mut actor: UIActor,
    terminal: &mut Terminal<B>,
) -> Result<(), Error> {
    loop {
        actor.draw(terminal)?;
        tokio::select! {
            Some(msg) = actor.receiver.recv() => {
                actor.handle_message(msg);
            }
            Some(Ok(event)) = actor.event_stream.next() => {
                if actor.handle_input_event(event).await.unwrap_or(false) {
                    break;
                }
            }
            else => {
                break;
            }
        }
    }

    Ok(())
}

#[derive(Clone, Debug)]
pub struct UIHandle {
    sender: Sender<UIMessage>,
}

type UIStarter<'a, B> = Box<
    dyn FnOnce(
        EventStream,
        AppHandle,
        &'a mut Terminal<B>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'a>>,
>;

impl UIHandle {
    pub fn new<'a, B: Backend>() -> (Self, UIStarter<'a, B>) {
        let (sender, receiver) = mpsc::channel(8);

        (
            Self { sender },
            Box::new(move |event_stream, app_handle, terminal| {
                let actor = UIActor::new(receiver, event_stream, app_handle);
                Box::pin(run_ui_actor(actor, terminal))
            }),
        )
    }

    pub async fn log(&self, message: String) -> Result<(), Error> {
        self.sender.send(UIMessage::Log(message)).await?;
        Ok(())
    }

    pub async fn turn_received(&self, new_sentence: String) -> Result<(), Error> {
        self.sender
            .send(UIMessage::SentenceReceived(new_sentence))
            .await?;
        Ok(())
    }

    pub async fn connected(&self, our_turn: bool) -> Result<(), Error> {
        self.sender.send(UIMessage::Connected(our_turn)).await?;
        Ok(())
    }

    pub async fn disconnected(&self) -> Result<(), Error> {
        self.sender.send(UIMessage::Disconnected).await?;
        Ok(())
    }
}
