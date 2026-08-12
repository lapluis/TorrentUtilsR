use std::cmp;
use std::collections::{HashMap, HashSet, hash_map::Entry};
use std::fs::{File, metadata};
use std::io::{Read, Seek, SeekFrom};
use std::path::{MAIN_SEPARATOR, Path, PathBuf};

use natord::compare_ignore_case;
use rayon::{ThreadPoolBuilder, prelude::*};
use sha1::{Digest, Sha1};
use walkdir::WalkDir;

use crate::bencode::{bencode_bytes, bencode_string, bencode_uint};
use crate::tr_file::{TrFile, bencode_file_list};
use crate::utils::{TrError, TrResult};

const SHA1_HASH_SIZE: usize = 20;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WalkMode {
    #[default]
    Default,
    Alphabetical,
    BreadthFirstAlphabetical, // tu like
    BreadthFirstLevel,        // qb like
    FileSize,
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerificationOptions {
    pub n_jobs: usize,
}

impl Default for VerificationOptions {
    fn default() -> Self {
        Self { n_jobs: 1 }
    }
}

struct FileHashInfo {
    file_index: usize,
    file_offset: usize,
    length: usize,
}

struct FailedInfo {
    files: HashSet<usize>,
    files_known: HashSet<usize>,
    pieces: HashSet<usize>,
}

/// Receives piece-processing progress without coupling the core library to a UI.
pub trait ProgressReporter: Sync {
    fn begin(&self, total: usize);
    fn advance(&self, delta: usize);
    fn finish(&self);
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

#[derive(Clone, Debug)]
pub struct TrInfo {
    pub files: Option<Vec<TrFile>>,
    pub length: Option<usize>,
    pub name: Option<String>,
    pub piece_length: usize,
    pub pieces: Vec<u8>,
    pub private: bool,
    pub source: Option<String>,
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
        let name = base_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                TrError::InvalidPath(format!(
                    "Invalid file name in path: {}",
                    base_path.display()
                ))
            })?;
        let mut single_file = false;

        let base_metadata = metadata(base_path)?;
        let mut tr_files: Vec<TrFile> = Vec::new();

        if base_metadata.is_file() {
            single_file = true;
            tr_files.push(TrFile {
                length: base_metadata.len() as usize,
                path: Vec::new(),
            });
        } else if base_metadata.is_dir() {
            for entry in WalkDir::new(base_path)
                .follow_links(true)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if entry.file_type().is_file() {
                    let entry_metadata = metadata(entry.path())?;
                    let relative_path = entry
                        .path()
                        .strip_prefix(base_path)
                        .map_err(|_| {
                            TrError::InvalidPath(String::from("Failed to create relative path"))
                        })?
                        .to_str()
                        .ok_or_else(|| {
                            TrError::InvalidPath(String::from("Path contains invalid UTF-8"))
                        })?
                        .split(MAIN_SEPARATOR)
                        .map(str::to_owned)
                        .collect();

                    tr_files.push(TrFile {
                        length: entry_metadata.len() as usize,
                        path: relative_path,
                    });
                }
            }
        } else {
            return Err(TrError::InvalidPath(String::from(
                "Target path is neither a file nor a directory",
            )));
        }

        match tr_config.walk_mode {
            WalkMode::Default => {}
            WalkMode::Alphabetical => {
                tr_files.sort_by(|a, b| a.path.cmp(&b.path));
            }
            WalkMode::BreadthFirstAlphabetical => {
                tr_files.sort_by(|a, b| {
                    a.path
                        .iter()
                        .zip(b.path.iter())
                        .find_map(|(seg_a, seg_b)| {
                            let cmp_res = compare_ignore_case(seg_a, seg_b);
                            (cmp_res != cmp::Ordering::Equal).then_some(cmp_res)
                        })
                        .unwrap_or_else(|| a.path.len().cmp(&b.path.len()))
                });
            }
            WalkMode::BreadthFirstLevel => {
                tr_files.sort_by(|a, b| {
                    a.path
                        .iter()
                        .zip(b.path.iter())
                        .enumerate()
                        .find_map(|(depth, (seg_a, seg_b))| {
                            match (depth == a.path.len() - 1, depth == b.path.len() - 1) {
                                (true, false) => Some(cmp::Ordering::Less),
                                (false, true) => Some(cmp::Ordering::Greater),
                                _ => {
                                    let cmp_res = compare_ignore_case(seg_a, seg_b);
                                    (cmp_res != cmp::Ordering::Equal).then_some(cmp_res)
                                }
                            }
                        })
                        .unwrap_or_else(|| a.path.len().cmp(&b.path.len()))
                });
            }
            WalkMode::FileSize => {
                tr_files.sort_by_key(|file| cmp::Reverse(file.length));
            }
        }

        let pieces = hash_tr_files(
            base_path,
            &tr_files,
            tr_config.piece_length,
            tr_config.n_jobs,
            progress,
        )?;

        Ok(TrInfo {
            files: if !single_file { Some(tr_files) } else { None },
            length: if single_file {
                Some(base_metadata.len() as usize)
            } else {
                None
            },
            name: Some(name.to_string()),
            piece_length: tr_config.piece_length,
            pieces,
            private: tr_config.private,
            source: tr_config.source.clone(),
        })
    }

    pub fn verify(
        &self,
        target_path: impl AsRef<Path>,
        options: &VerificationOptions,
        progress: Option<&dyn ProgressReporter>,
    ) -> TrResult<VerificationReport> {
        if self.piece_length == 0 {
            return Err(TrError::InvalidTorrent(String::from(
                "piece length must be greater than zero",
            )));
        }
        if options.n_jobs == 0 {
            return Err(TrError::InvalidConfig(String::from(
                "number of jobs must be greater than zero",
            )));
        }
        if !self.pieces.len().is_multiple_of(SHA1_HASH_SIZE) {
            return Err(TrError::InvalidTorrent(String::from(
                "pieces length is not a multiple of the SHA-1 hash size",
            )));
        }

        let base_path = target_path.as_ref();
        let single_file;
        let tr_files: &[TrFile] = match &self.files {
            Some(files) => files,
            None => {
                single_file = vec![TrFile {
                    length: self
                        .length
                        .ok_or_else(|| TrError::MissingField(String::from("length")))?,
                    path: Vec::new(),
                }];
                &single_file
            }
        };

        let piece_slices: Vec<[u8; SHA1_HASH_SIZE]> = split_hash_pieces(&self.pieces);
        let expected_piece_count = tr_files
            .iter()
            .try_fold(0usize, |total, file| total.checked_add(file.length))
            .ok_or_else(|| TrError::InvalidTorrent(String::from("total file size overflow")))?
            .div_ceil(self.piece_length);
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
            self.piece_length,
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
                let path = if tr_file.path.is_empty() {
                    self.name
                        .clone()
                        .ok_or_else(|| TrError::MissingField(String::from("name")))?
                } else {
                    tr_file.path.join("/")
                };
                Ok(FailedFile {
                    index: file_index,
                    path,
                    length: tr_file.length,
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

    pub fn get_name(&self) -> TrResult<String> {
        self.name
            .clone()
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

fn hash_tr_files(
    base_path: &Path,
    tr_files: &[TrFile],
    chunk_size: usize,
    n_jobs: usize,
    progress: Option<&dyn ProgressReporter>,
) -> TrResult<Vec<u8>> {
    let piece_file_info = calc_piece_file_info(tr_files, chunk_size);
    let pieces_count = piece_file_info.len();

    if let Some(progress) = progress {
        progress.begin(pieces_count);
    }

    let piece_slices = hash_piece_file(
        chunk_size,
        &piece_file_info,
        tr_files,
        base_path,
        progress,
        n_jobs,
    )?;

    let mut pieces = Vec::with_capacity(piece_slices.len() * SHA1_HASH_SIZE);
    for slice in piece_slices {
        pieces.extend_from_slice(&slice);
    }

    if let Some(progress) = progress {
        progress.finish();
    }

    Ok(pieces)
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

    if let Some(progress) = progress {
        progress.begin(pieces_count);
    }

    for (i, piece) in piece_file_info.iter().enumerate() {
        let mut files_ok: bool = true;
        for file_hash_info in piece {
            let tr_file = &tr_files[file_hash_info.file_index];
            let f_path = tr_file.join_full_path(base_path);
            match file_status_map.entry(f_path) {
                Entry::Vacant(entry) => {
                    let file_ok = metadata(entry.key())
                        .ok()
                        .is_some_and(|meta| meta.len() == tr_file.length as u64);
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
            if let Some(progress) = progress {
                progress.advance(1);
            }
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
        progress,
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

    if let Some(progress) = progress {
        progress.finish();
    }

    Ok(failed_info)
}

fn split_hash_pieces(piece: &[u8]) -> Vec<[u8; SHA1_HASH_SIZE]> {
    let layer_count = piece.len() / SHA1_HASH_SIZE;
    let mut slices: Vec<[u8; SHA1_HASH_SIZE]> = vec![[0u8; SHA1_HASH_SIZE]; layer_count];
    for i in 0..layer_count {
        slices[i].copy_from_slice(&piece[i * SHA1_HASH_SIZE..(i + 1) * SHA1_HASH_SIZE]);
    }
    slices
}

fn calc_piece_file_info(tr_files: &[TrFile], piece_length: usize) -> Vec<Vec<FileHashInfo>> {
    let total_size: usize = tr_files.iter().map(|f| f.length).sum();
    let pieces_count = total_size.div_ceil(piece_length);

    let mut piece_file_info: Vec<Vec<FileHashInfo>> = Vec::with_capacity(pieces_count);
    let mut unfilled_size = 0usize;

    for (file_index, tr_file) in tr_files.iter().enumerate() {
        let mut rest_size = tr_file.length;
        let mut file_offset = 0usize;
        while rest_size > 0 {
            if unfilled_size == 0 {
                piece_file_info.push(Vec::new());
                unfilled_size = piece_length;
            }
            let used_size = cmp::min(rest_size, unfilled_size);
            piece_file_info
                .last_mut()
                .expect("Piece file info should have at least one piece")
                .push(FileHashInfo {
                    file_index,
                    file_offset,
                    length: used_size,
                });
            file_offset += used_size;
            rest_size -= used_size;
            unfilled_size -= used_size;
        }
    }

    piece_file_info
}

struct SequentialFileReader {
    file_index: Option<usize>,
    file: Option<File>,
    next_offset: u64,
}

impl SequentialFileReader {
    const fn new() -> Self {
        Self {
            file_index: None,
            file: None,
            next_offset: 0,
        }
    }

    fn read_exact_at(
        &mut self,
        file_index: usize,
        path: &Path,
        offset: u64,
        buf: &mut [u8],
    ) -> TrResult<()> {
        if self.file_index != Some(file_index) {
            self.file = Some(File::open(path)?);
            self.file_index = Some(file_index);
            self.next_offset = 0;
        }

        let file = self.file.as_mut().expect("file was opened above");
        if self.next_offset != offset {
            file.seek(SeekFrom::Start(offset))?;
        }
        file.read_exact(buf)?;
        self.next_offset = offset + buf.len() as u64;
        Ok(())
    }
}

fn hash_piece_file(
    piece_length: usize,
    piece_file_info: &[Vec<FileHashInfo>],
    tr_files: &[TrFile],
    base_path: &Path,
    progress: Option<&dyn ProgressReporter>,
    n_jobs: usize,
) -> TrResult<Vec<[u8; SHA1_HASH_SIZE]>> {
    if piece_file_info.is_empty() {
        return Ok(Vec::new());
    }

    let f_path_list: Vec<_> = tr_files
        .iter()
        .map(|tr_file| tr_file.join_full_path(base_path))
        .collect();

    // More chunks than workers retain Rayon's load balancing while each chunk still
    // processes enough adjacent pieces to reuse its current file handle efficiently.
    const CHUNKS_PER_WORKER: usize = 4;
    let chunk_count = n_jobs
        .saturating_mul(CHUNKS_PER_WORKER)
        .min(piece_file_info.len())
        .max(1);
    let chunk_size = piece_file_info.len().div_ceil(chunk_count);

    let results: Result<Vec<[u8; SHA1_HASH_SIZE]>, TrError> = {
        let pool = ThreadPoolBuilder::new()
            .num_threads(n_jobs)
            .build()
            .map_err(|e| TrError::ParseError(format!("Failed to create thread pool: {e}")))?;

        pool.install(|| {
            piece_file_info
                .par_chunks(chunk_size)
                .map(|piece_chunk| -> TrResult<Vec<[u8; SHA1_HASH_SIZE]>> {
                    let mut reader = SequentialFileReader::new();
                    let mut buf = vec![0; piece_length];
                    let mut hashes = Vec::with_capacity(piece_chunk.len());

                    for piece in piece_chunk {
                        let mut hasher = Sha1::new();
                        for file_hash_info in piece {
                            let f_path = &f_path_list[file_hash_info.file_index];
                            let buf_slice = &mut buf[..file_hash_info.length];
                            reader.read_exact_at(
                                file_hash_info.file_index,
                                f_path,
                                file_hash_info.file_offset as u64,
                                buf_slice,
                            )?;
                            hasher.update(buf_slice);
                        }

                        let calc_hash = hasher.finalize();
                        let mut hash = [0u8; SHA1_HASH_SIZE];
                        hash.copy_from_slice(&calc_hash);
                        hashes.push(hash);

                        if let Some(progress) = progress {
                            progress.advance(1);
                        }
                    }

                    Ok(hashes)
                })
                .collect::<TrResult<Vec<_>>>()
                .map(|chunks| chunks.into_iter().flatten().collect())
        })
    };

    results
}
