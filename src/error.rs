use std::error;
use std::fmt::{Display, Formatter, Result as fmtResult};
use std::io::Error as ioError;
use std::string::FromUtf8Error;

#[derive(Debug)]
pub enum BencodeError {
    UnexpectedEof,
    UnterminatedInteger,
    InvalidInteger,
    InvalidUtf8(&'static str),
    UnexpectedToken(u8),
    TruncatedStringLength,
    InvalidStringLength,
    StringLengthOverflow,
    TruncatedString,
    DictKeyNotString,
    TrailingData,
}

impl Display for BencodeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmtResult {
        match self {
            Self::UnexpectedEof => write!(f, "unexpected EOF"),
            Self::UnterminatedInteger => write!(f, "unterminated integer"),
            Self::InvalidInteger => write!(f, "invalid integer"),
            Self::InvalidUtf8(context) => write!(f, "invalid UTF-8 in {context}"),
            Self::UnexpectedToken(byte) => write!(f, "unexpected token: {byte}"),
            Self::TruncatedStringLength => write!(f, "truncated string length"),
            Self::InvalidStringLength => write!(f, "invalid string length"),
            Self::StringLengthOverflow => write!(f, "string length overflow"),
            Self::TruncatedString => write!(f, "truncated string"),
            Self::DictKeyNotString => write!(f, "dictionary key is not a string"),
            Self::TrailingData => write!(f, "trailing data after torrent"),
        }
    }
}

impl error::Error for BencodeError {}

#[derive(Debug)]
pub enum TrError {
    IO(ioError),
    InvalidConfig(String),
    InvalidPath(String),
    InvalidTorrent(String),
    MissingField(&'static str),
    Bencode(BencodeError),
    ThreadPool(String),
    Encoding(FromUtf8Error),
}

impl Display for TrError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmtResult {
        match self {
            TrError::IO(err) => write!(f, "IO error: {err}"),
            TrError::InvalidConfig(msg) => write!(f, "Invalid configuration: {msg}"),
            TrError::InvalidPath(path) => write!(f, "Invalid path: {path}"),
            TrError::InvalidTorrent(msg) => write!(f, "Invalid torrent: {msg}"),
            TrError::MissingField(field) => write!(f, "Missing field: {field}"),
            TrError::Bencode(err) => write!(f, "Parse error: {err}"),
            TrError::ThreadPool(msg) => write!(f, "Thread pool error: {msg}"),
            TrError::Encoding(err) => write!(f, "Encoding error: UTF-8 conversion error: {err}"),
        }
    }
}

impl error::Error for TrError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            TrError::IO(err) => Some(err),
            TrError::Bencode(err) => Some(err),
            TrError::Encoding(err) => Some(err),
            _ => None,
        }
    }
}

impl From<ioError> for TrError {
    fn from(err: ioError) -> Self {
        TrError::IO(err)
    }
}

impl From<FromUtf8Error> for TrError {
    fn from(err: FromUtf8Error) -> Self {
        TrError::Encoding(err)
    }
}

impl From<BencodeError> for TrError {
    fn from(err: BencodeError) -> Self {
        TrError::Bencode(err)
    }
}

pub type TrResult<T> = Result<T, TrError>;
