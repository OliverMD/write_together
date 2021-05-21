use std::io;

use crate::{app::AppHandle, error::Error, ui_actor::UIHandle};
use clap::Clap;
use crossterm::{
    event::EventStream,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use tui::{backend::CrosstermBackend, Terminal};

mod app;
mod error;
mod ui_actor;

#[derive(Clap)]
struct Opts {
    #[clap(short, long)]
    port: u16,
}

#[tokio::main]
pub async fn main() -> Result<(), Error> {
    let opts = Opts::parse();

    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();
    enable_raw_mode().unwrap();
    terminal.clear().unwrap();

    let reader = EventStream::new();

    {
        let (ui_handle, ui_starter) = UIHandle::new();
        let app_handle = AppHandle::new(opts.port, ui_handle);
        ui_starter(reader, app_handle, &mut terminal).await?;
    }

    disable_raw_mode().unwrap();
    terminal.clear().unwrap();
    Ok(())
}
