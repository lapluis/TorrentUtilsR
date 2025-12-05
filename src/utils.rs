use indicatif::{ProgressBar, ProgressStyle};
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
    const UNITS: &[(usize, &str)] = &[
        (1024 * 1024 * 1024, "GiB"),
        (1024 * 1024, "MiB"),
        (1024, "KiB"),
    ];

    for &(unit_size, unit_name) in UNITS {
        if bytes >= unit_size {
            return if bytes.is_multiple_of(unit_size) {
                format!("{} {}", bytes / unit_size, unit_name)
            } else {
                let value = bytes as f64 / unit_size as f64;
                format!("{value:.2} {unit_name}")
            };
        }
    }

    format!("{bytes} B")
}

pub fn make_progress_bar(total: usize, quiet: bool) -> Option<ProgressBar> {
    if quiet {
        None
    } else {
        let pb = ProgressBar::new(total as u64);
        pb.set_style(
            ProgressStyle::with_template("{spinner:.green} [{bar:40.cyan/blue}] [{pos}/{len}] pieces ({percent}%, eta: {eta})")
                .unwrap()
                .progress_chars("#>-"),
        );
        Some(pb)
    }
}

pub fn finish_progress_bar(pb: Option<ProgressBar>, pieces_count: usize) {
    if let Some(pb) = pb {
        let elapsed = pb.elapsed();
        pb.finish_and_clear();
        println!(
            "\x1b[32m✓\x1b[0m [\x1b[36m########################################\x1b[0m] [{pieces_count}/{pieces_count}] pieces (100%, eta: 0s)"
        );
        println!("Processed {pieces_count} pieces in {elapsed:.2?}");
    }
}
