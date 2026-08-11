//! Core BitTorrent creation, parsing, serialization, and verification APIs.

mod bencode;
mod torrent;
mod tr_file;
mod tr_info;
mod utils;

pub use torrent::Torrent;
pub use tr_file::{FileTree, TrFile};
pub use tr_info::{
    CreateOptions, FailedFile, ProgressReporter, TrConfig, TrInfo, VerificationOptions,
    VerificationReport, WalkMode,
};
pub use utils::{TrError, TrResult, human_size};
