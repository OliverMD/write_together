use std::fmt::{Debug, Display, Formatter};
use tokio::sync::mpsc::error::SendError;

#[derive(Debug)]
pub enum Error {
    IO(std::io::Error),
    Send(Box<dyn std::error::Error + Send>),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::IO(err) => write!(f, "IO error: {}", err),
            Error::Send(err) => write!(f, "Send error: {}", err),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::IO(err)
    }
}

impl<T: 'static + Debug + Display + Send> From<SendError<T>> for Error {
    fn from(err: SendError<T>) -> Self {
        Error::Send(Box::new(err))
    }
}
