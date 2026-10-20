use std::collections::{HashMap, HashSet, hash_map::Entry};
use std::fs::metadata;
use std::path::{Path, PathBuf};

use crate::error::{TrError, TrResult};
use crate::hash::{SHA1_HASH_SIZE, hash_piece_file, split_hash_pieces};
use crate::piece::calc_piece_file_info;
use crate::progress::{ProgressGuard, ProgressReporter};
use crate::tr_file::TrFile;
use crate::tr_info::TrInfo;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerificationOptions {
    pub n_jobs: usize,
}

impl Default for VerificationOptions {
    fn default() -> Self {
        Self { n_jobs: 1 }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailedFile {
    pub index: usize,
    pub path: String,
    pub length: usize,
    pub missing_or_size_mismatch: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationReport {
    pub total_pieces: usize,
    pub failed_pieces: Vec<usize>,
    pub total_files: usize,
    pub failed_files: Vec<FailedFile>,
}

impl VerificationReport {
    pub fn passed_pieces(&self) -> usize {
        self.total_pieces.saturating_sub(self.failed_pieces.len())
    }

    pub fn passed_files(&self) -> usize {
        self.total_files.saturating_sub(self.failed_files.len())
    }

    pub fn is_ok(&self) -> bool {
        self.failed_pieces.is_empty() && self.failed_files.is_empty()
    }
}

struct FailedInfo {
    files: HashSet<usize>,
    files_known: HashSet<usize>,
    pieces: HashSet<usize>,
}

impl TrInfo {
    pub fn verify(
        &self,
        target_path: impl AsRef<Path>,
        options: &VerificationOptions,
        progress: Option<&dyn ProgressReporter>,
    ) -> TrResult<VerificationReport> {
        if self.piece_length() == 0 {
            return Err(TrError::InvalidTorrent(String::from(
                "piece length must be greater than zero",
            )));
        }
        if options.n_jobs == 0 {
            return Err(TrError::InvalidConfig(String::from(
                "number of jobs must be greater than zero",
            )));
        }
        if !self.pieces().len().is_multiple_of(SHA1_HASH_SIZE) {
            return Err(TrError::InvalidTorrent(String::from(
                "pieces length is not a multiple of the SHA-1 hash size",
            )));
        }

        let base_path = target_path.as_ref();
        let single_file;
        let tr_files: &[TrFile] = match self.files() {
            Some(files) => files,
            None => {
                single_file = vec![TrFile::new(
                    self.length().ok_or(TrError::MissingField("length"))?,
                    Vec::new(),
                )];
                &single_file
            }
        };

        let piece_slices = split_hash_pieces(self.pieces());
        let expected_piece_count = tr_files
            .iter()
            .try_fold(0usize, |total, file| total.checked_add(file.length()))
            .ok_or_else(|| TrError::InvalidTorrent(String::from("total file size overflow")))?
            .div_ceil(self.piece_length());
        if piece_slices.len() != expected_piece_count {
            return Err(TrError::InvalidTorrent(format!(
                "piece count mismatch: expected {expected_piece_count}, found {}",
                piece_slices.len()
            )));
        }

        let failed_info = verify_tr_files(
            &piece_slices,
            tr_files,
            base_path,
            self.piece_length(),
            options.n_jobs,
            progress,
        )?;

        let total_pieces = piece_slices.len();
        let total_files = tr_files.len();
        let mut failed_pieces: Vec<usize> = failed_info.pieces.iter().copied().collect();
        failed_pieces.sort_unstable();
        let mut failed_file_indexes: Vec<usize> = failed_info.files.iter().copied().collect();
        failed_file_indexes.sort_unstable();
        let failed_files = failed_file_indexes
            .into_iter()
            .map(|file_index| {
                let tr_file = &tr_files[file_index];
                let path = if tr_file.path().is_empty() {
                    self.get_name()?
                } else {
                    tr_file.path().join("/")
                };
                Ok(FailedFile {
                    index: file_index,
                    path,
                    length: tr_file.length(),
                    missing_or_size_mismatch: failed_info.files_known.contains(&file_index),
                })
            })
            .collect::<TrResult<Vec<_>>>()?;

        Ok(VerificationReport {
            total_pieces,
            failed_pieces,
            total_files,
            failed_files,
        })
    }
}

fn verify_tr_files(
    piece_slices: &[[u8; SHA1_HASH_SIZE]],
    tr_files: &[TrFile],
    base_path: &Path,
    piece_length: usize,
    n_jobs: usize,
    progress: Option<&dyn ProgressReporter>,
) -> TrResult<FailedInfo> {
    let piece_file_info = calc_piece_file_info(tr_files, piece_length);

    let mut file_status_map: HashMap<PathBuf, bool> = HashMap::new();
    let mut failed_info = FailedInfo {
        files: HashSet::new(),
        files_known: HashSet::new(),
        pieces: HashSet::new(),
    };
    let pieces_count = piece_slices.len();
    let mut progress = ProgressGuard::begin(progress, pieces_count);

    for (i, piece) in piece_file_info.iter().enumerate() {
        let mut files_ok: bool = true;
        for file_hash_info in piece {
            let tr_file = &tr_files[file_hash_info.file_index];
            let f_path = tr_file.join_full_path(base_path);
            match file_status_map.entry(f_path) {
                Entry::Vacant(entry) => {
                    let file_ok = metadata(entry.key())
                        .ok()
                        .is_some_and(|meta| meta.len() == tr_file.length() as u64);
                    if !file_ok {
                        failed_info.files_known.insert(file_hash_info.file_index);
                        files_ok = false;
                    }
                    entry.insert(file_ok);
                }
                Entry::Occupied(entry) => {
                    if !*entry.get() {
                        files_ok = false;
                    }
                }
            }
        }
        if !files_ok {
            failed_info.pieces.insert(i);
            for file_hash_info in piece {
                failed_info.files.insert(file_hash_info.file_index);
            }
            progress.advance(1);
            continue;
        }
    }

    let pieces_to_check_count = pieces_count - failed_info.pieces.len();
    let mut pieces_to_check = Vec::with_capacity(pieces_to_check_count);
    let mut filtered_piece_file_info = Vec::with_capacity(pieces_to_check_count);
    for (i, piece_info) in piece_file_info.into_iter().enumerate() {
        if !failed_info.pieces.contains(&i) {
            pieces_to_check.push(i);
            filtered_piece_file_info.push(piece_info);
        }
    }
    let piece_file_info = filtered_piece_file_info;

    let calc_piece_slices = hash_piece_file(
        piece_length,
        &piece_file_info,
        tr_files,
        base_path,
        &progress,
        n_jobs,
    )?;
    for (i, piece_calc_hash) in calc_piece_slices.iter().enumerate() {
        if *piece_calc_hash != piece_slices[pieces_to_check[i]] {
            failed_info.pieces.insert(pieces_to_check[i]);
            for file_hash_info in &piece_file_info[i] {
                failed_info.files.insert(file_hash_info.file_index);
            }
        }
    }

    progress.finish();

    Ok(failed_info)
}
