use std::path::Path;

use sha1::{Digest, Sha1};

use crate::codec::{bencode_bytes, bencode_string, bencode_uint};
use crate::hash::{SHA1_HASH_SIZE, hash_tr_files};
use crate::progress::ProgressReporter;
use crate::tr_file::{TrFile, bencode_file_list};
use crate::utils::{TrError, TrResult};
use crate::walk::{WalkMode, walk_target};

#[derive(Clone, Debug)]
pub struct CreateOptions {
    pub piece_length: usize,
    pub private: bool,
    pub n_jobs: usize,
    pub walk_mode: WalkMode,
    pub source: Option<String>,
}

/// Backwards-compatible name for [`CreateOptions`].
pub type TrConfig = CreateOptions;

#[derive(Clone, Debug)]
pub struct TrInfo {
    files: Option<Vec<TrFile>>,
    length: Option<usize>,
    name: Option<String>,
    piece_length: usize,
    pieces: Vec<u8>,
    private: bool,
    source: Option<String>,
}

fn is_safe_path_segment(segment: &str) -> bool {
    if segment.is_empty() || segment == "." || segment == ".." || segment.contains('\0') {
        return false;
    }
    if segment.contains('/') {
        return false;
    }
    #[cfg(windows)]
    if segment.contains('\\') || segment.contains(':') {
        return false;
    }
    true
}

impl TrInfo {
    pub fn new(
        target_path: impl AsRef<Path>,
        tr_config: &CreateOptions,
        progress: Option<&dyn ProgressReporter>,
    ) -> TrResult<TrInfo> {
        if tr_config.piece_length == 0 {
            return Err(TrError::InvalidConfig(String::from(
                "piece length must be greater than zero",
            )));
        }
        if tr_config.n_jobs == 0 {
            return Err(TrError::InvalidConfig(String::from(
                "number of jobs must be greater than zero",
            )));
        }

        let base_path = target_path.as_ref();
        let walked = walk_target(base_path, tr_config.walk_mode)?;
        let pieces = hash_tr_files(
            base_path,
            &walked.files,
            tr_config.piece_length,
            tr_config.n_jobs,
            progress,
        )?;

        let length = if walked.single_file {
            Some(walked.files[0].length())
        } else {
            None
        };
        let files = if walked.single_file {
            None
        } else {
            Some(walked.files)
        };

        TrInfo::from_parts(
            files,
            length,
            Some(walked.name),
            tr_config.piece_length,
            pieces,
            tr_config.private,
            tr_config.source.clone(),
        )
    }

    /// Builds a validated `TrInfo` from parsed or externally supplied fields.
    ///
    /// Validates the name and file paths, the `files`/`length` invariant, the
    /// piece length, and that the piece data matches the file sizes.
    pub fn from_parts(
        files: Option<Vec<TrFile>>,
        length: Option<usize>,
        name: Option<String>,
        piece_length: usize,
        pieces: Vec<u8>,
        private: bool,
        source: Option<String>,
    ) -> TrResult<Self> {
        if piece_length == 0 {
            return Err(TrError::InvalidTorrent(String::from(
                "piece length must be greater than zero",
            )));
        }

        let name = name.ok_or_else(|| TrError::MissingField(String::from("name")))?;
        if !is_safe_path_segment(&name) {
            return Err(TrError::InvalidTorrent(format!(
                "invalid torrent name: {name}"
            )));
        }

        if files.is_some() == length.is_some() {
            return Err(TrError::InvalidTorrent(String::from(
                "torrent must contain exactly one of files or length",
            )));
        }

        if let Some(files) = &files {
            for file in files {
                let path = file.path();
                if path.is_empty() || !path.iter().all(|segment| is_safe_path_segment(segment)) {
                    return Err(TrError::InvalidTorrent(format!(
                        "invalid file path: {}",
                        path.join("/")
                    )));
                }
            }
        }

        if !pieces.len().is_multiple_of(SHA1_HASH_SIZE) {
            return Err(TrError::InvalidTorrent(String::from(
                "pieces length is not a multiple of the SHA-1 hash size",
            )));
        }

        let total_length = if let Some(files) = &files {
            files
                .iter()
                .try_fold(0usize, |total, file| total.checked_add(file.length()))
                .ok_or_else(|| TrError::InvalidTorrent(String::from("total file size overflow")))?
        } else {
            length.unwrap_or_default()
        };

        let expected_piece_count = total_length.div_ceil(piece_length);
        let found_piece_count = pieces.len() / SHA1_HASH_SIZE;
        if found_piece_count != expected_piece_count {
            return Err(TrError::InvalidTorrent(format!(
                "piece count mismatch: expected {expected_piece_count}, found {found_piece_count}"
            )));
        }

        Ok(Self {
            files,
            length,
            name: Some(name),
            piece_length,
            pieces,
            private,
            source,
        })
    }

    pub fn files(&self) -> Option<&[TrFile]> {
        self.files.as_deref()
    }

    pub fn length(&self) -> Option<usize> {
        self.length
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn piece_length(&self) -> usize {
        self.piece_length
    }

    pub fn pieces(&self) -> &[u8] {
        &self.pieces
    }

    pub fn is_private(&self) -> bool {
        self.private
    }

    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    pub fn get_name(&self) -> TrResult<String> {
        self.name()
            .map(str::to_owned)
            .ok_or_else(|| TrError::MissingField(String::from("name")))
    }

    pub fn bencode(&self) -> Vec<u8> {
        let mut bcode: Vec<u8> = Vec::new();
        bcode.push(b'd');
        if let Some(files) = &self.files {
            bcode.extend(bencode_string("files"));
            bcode.extend(bencode_file_list(files));
        }
        if let Some(length) = self.length {
            bcode.extend(bencode_string("length"));
            bcode.extend(bencode_uint(length));
        }
        if let Some(name) = &self.name {
            bcode.extend(bencode_string("name"));
            bcode.extend(bencode_string(name));
        }
        bcode.extend(bencode_string("piece length"));
        bcode.extend(bencode_uint(self.piece_length));
        bcode.extend(bencode_string("pieces"));
        bcode.extend(bencode_bytes(&self.pieces));
        if self.private {
            bcode.extend(bencode_string("private"));
            bcode.extend(bencode_uint(1));
        }
        if let Some(source) = &self.source {
            bcode.extend(bencode_string("source"));
            bcode.extend(bencode_string(source));
        }
        bcode.push(b'e');
        bcode
    }

    pub fn hash(&self) -> String {
        let mut hasher = Sha1::new();
        hasher.update(self.bencode());
        let result = hasher.finalize();
        hex::encode(result)
    }
}
