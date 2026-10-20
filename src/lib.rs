//! Core BitTorrent creation, parsing, serialization, and verification APIs.

mod codec;
mod error;
mod hash;
mod piece;
mod progress;
mod torrent;
mod tr_file;
mod tr_info;
mod verify;
mod walk;

pub use error::{BencodeError, TrError, TrResult};
pub use progress::ProgressReporter;
pub use torrent::Torrent;
pub use tr_file::TrFile;
pub use tr_info::{CreateOptions, TrConfig, TrInfo};
pub use verify::{FailedFile, VerificationOptions, VerificationReport};
pub use walk::WalkMode;
