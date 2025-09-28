use std::fmt::{Display, Formatter, Result as fmtResult};
use std::io::Error as ioError;
use std::{error, string};

#[derive(Debug)]
pub enum TrError {
    IO(ioError),
    InvalidPath(String),
    InvalidTorrent(String),
    MissingField(String),
    ParseError(String),
    EncodingError(String),
}

impl Display for TrError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmtResult {
        match self {
            TrError::IO(err) => write!(f, "IO error: {err}"),
            TrError::InvalidPath(path) => write!(f, "Invalid path: {path}"),
            TrError::InvalidTorrent(msg) => write!(f, "Invalid torrent: {msg}"),
            TrError::MissingField(field) => write!(f, "Missing field: {field}"),
            TrError::ParseError(msg) => write!(f, "Parse error: {msg}"),
            TrError::EncodingError(msg) => write!(f, "Encoding error: {msg}"),
        }
    }
}

impl error::Error for TrError {}

impl From<ioError> for TrError {
    fn from(err: ioError) -> Self {
        TrError::IO(err)
    }
}

impl From<string::FromUtf8Error> for TrError {
    fn from(err: string::FromUtf8Error) -> Self {
        TrError::EncodingError(format!("UTF-8 conversion error: {err}"))
    }
}

impl From<&str> for TrError {
    fn from(err: &str) -> Self {
        TrError::ParseError(err.to_string())
    }
}

impl From<String> for TrError {
    fn from(err: String) -> Self {
        TrError::ParseError(err)
    }
}

pub type TrResult<T> = Result<T, TrError>;

pub fn human_size(bytes: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = 1024 * KB;
    const GB: usize = 1024 * MB;

    if bytes >= GB {
        let whole = bytes / GB;
        let remainder = bytes % GB;
        if remainder == 0 {
            format!("{whole} GiB")
        } else {
            let value = bytes as f64 / GB as f64;
            format!("{value:.2} GiB")
        }
    } else if bytes >= MB {
        let whole = bytes / MB;
        let remainder = bytes % MB;
        if remainder == 0 {
            format!("{whole} MiB")
        } else {
            let value = bytes as f64 / MB as f64;
            format!("{value:.2} MiB")
        }
    } else if bytes >= KB {
        let whole = bytes / KB;
        let remainder = bytes % KB;
        if remainder == 0 {
            format!("{whole} KiB")
        } else {
            let value = bytes as f64 / KB as f64;
            format!("{value:.2} KiB")
        }
    } else {
        format!("{bytes} B")
    }
}
