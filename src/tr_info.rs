use std::cell::RefCell;
use std::cmp;
use std::collections::{HashMap, HashSet, hash_map::Entry};
use std::fs::{File, metadata};
use std::io::{Read, Seek, SeekFrom};
use std::path::{MAIN_SEPARATOR, Path};

use indicatif::{ProgressBar, ProgressStyle};
use natord::compare_ignore_case;
use rayon::{ThreadPoolBuilder, prelude::*};
use sha1::{Digest, Sha1};
use walkdir::WalkDir;

use crate::bencode::{bencode_bytes, bencode_string, bencode_uint};
use crate::torrent::WalkMode;
use crate::tr_file::{TrFile, bencode_file_list};
use crate::utils::{TrError, TrResult, human_size};

const SHA1_HASH_SIZE: usize = 20;

const PB_STYLE_TEMPLATE: &str =
    "{spinner:.green} [{bar:40.cyan/blue}] [{pos}/{len}] pieces ({percent}%, eta: {eta})";
const PB_PROGRESS_CHARS: &str = "#>-";
const FINISHED_LINE_PREFIX: &str =
    "\x1b[32m✓\x1b[0m [\x1b[36m########################################\x1b[0m]";
const FINISHED_LINE_SUFFIX: &str = "pieces (100%, eta: 0s)";

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

pub struct TrInfo {
    pub files: Option<Vec<TrFile>>,
    pub length: Option<usize>,
    pub name: Option<String>,
    pub piece_length: usize,
    pub pieces: Vec<u8>,
    pub private: bool,
}

impl TrInfo {
    pub fn new(
        target_path: String,
        piece_length: usize,
        private: bool,
        n_jobs: usize,
        quiet: bool,
        walk_mode: WalkMode,
    ) -> TrResult<TrInfo> {
        let base_path = Path::new(&target_path);
        let name = base_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                TrError::InvalidPath(format!("Invalid file name in path: {target_path}"))
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

        match walk_mode {
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
                tr_files.sort_by(|a, b| b.length.cmp(&a.length));
            }
        }

        let pieces = hash_tr_files(base_path, &tr_files, piece_length, n_jobs, quiet)?;

        Ok(TrInfo {
            files: if !single_file { Some(tr_files) } else { None },
            length: if single_file {
                Some(base_metadata.len() as usize)
            } else {
                None
            },
            name: Some(name.to_string()),
            piece_length,
            pieces,
            private,
        })
    }

    pub fn verify(&self, target_path: String, n_jobs: usize, quiet: bool) -> TrResult<()> {
        let base_path = Path::new(&target_path);
        let tr_files = match self.files {
            Some(ref files) => files,
            None => &vec![TrFile {
                length: self
                    .length
                    .ok_or_else(|| TrError::MissingField(String::from("length")))?,
                path: Vec::new(),
            }],
        };

        let piece_slices: Vec<[u8; SHA1_HASH_SIZE]> = split_hash_pieces(&self.pieces);

        let failed_info = verify_tr_files(
            &piece_slices,
            tr_files,
            base_path,
            self.piece_length,
            n_jobs,
            quiet,
        )?;

        println!("Verification Result:");

        let total_pieces = piece_slices.len();
        let failed_piece_count = failed_info.pieces.len();
        let passed_piece_count = total_pieces - failed_piece_count;

        let total_files = tr_files.len();
        let failed_file_count = failed_info.files.len();
        let passed_file_count = total_files - failed_file_count;

        println!(
            "Pieces: {total_pieces:8} total = {passed_piece_count:8} passed + {failed_piece_count:8} failed"
        );
        println!(
            "Files:  {total_files:8} total = {passed_file_count:8} passed + {failed_file_count:8} failed"
        );

        if failed_info.files.is_empty() {
            println!("All files are OK.");
        } else {
            println!("\nSome files failed verification:");
            let mut failed_files_vec: Vec<usize> = failed_info.files.iter().cloned().collect();
            failed_files_vec.sort();
            for file_index in failed_files_vec {
                let tr_file = &tr_files[file_index];
                let rel_path = if tr_file.path.is_empty() {
                    self.name
                        .as_ref()
                        .ok_or_else(|| TrError::MissingField(String::from("name")))?
                        .to_string()
                } else {
                    tr_file.path.join("/")
                };
                let known_issue = if failed_info.files_known.contains(&file_index) {
                    " [missing or size mismatch]"
                } else {
                    ""
                };
                println!(
                    "- {} ({} [{}]){}",
                    rel_path,
                    tr_file.length,
                    human_size(tr_file.length),
                    known_issue
                );
            }
        }
        Ok(())
    }

    pub fn get_name(&self) -> TrResult<String> {
        self.name
            .clone()
            .ok_or_else(|| TrError::MissingField(String::from("name")))
    }

    pub fn bencode(&self) -> Vec<u8> {
        let mut bcode: Vec<u8> = Vec::new();
        bcode.push(b'd');
        if self.files.is_some() {
            bcode.extend(bencode_string("files"));
            bcode.extend(bencode_file_list(self.files.as_ref().unwrap()));
        }
        if self.length.is_some() {
            bcode.extend(bencode_string("length"));
            bcode.extend(bencode_uint(self.length.unwrap()));
        }
        if self.name.is_some() {
            bcode.extend(bencode_string("name"));
            bcode.extend(bencode_string(self.name.as_ref().unwrap()));
        }
        bcode.extend(bencode_string("piece length"));
        bcode.extend(bencode_uint(self.piece_length));
        if !self.pieces.is_empty() {
            bcode.extend(bencode_string("pieces"));
            bcode.extend(bencode_bytes(&self.pieces));
        }
        if self.private {
            bcode.extend(bencode_string("private"));
            bcode.extend(bencode_uint(1));
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
    quiet: bool,
) -> TrResult<Vec<u8>> {
    let piece_file_info = calc_piece_file_info(tr_files, chunk_size);
    let pieces_count = piece_file_info.len();

    let pb = if !quiet {
        let pb = ProgressBar::new(pieces_count as u64);
        pb.set_style(
            ProgressStyle::with_template(PB_STYLE_TEMPLATE)
                .unwrap()
                .progress_chars(PB_PROGRESS_CHARS),
        );
        Some(pb)
    } else {
        None
    };

    let piece_slices = hash_piece_file(
        chunk_size,
        &piece_file_info,
        tr_files,
        base_path,
        &pb,
        n_jobs,
    )?;

    let mut pieces = Vec::with_capacity(piece_slices.len() * SHA1_HASH_SIZE);
    for slice in piece_slices {
        pieces.extend_from_slice(&slice);
    }

    if let Some(pb) = pb {
        let elapsed = pb.elapsed();
        pb.finish_and_clear();
        println!("{FINISHED_LINE_PREFIX} [{pieces_count}/{pieces_count}] {FINISHED_LINE_SUFFIX}");
        println!("Processed {pieces_count} pieces in {elapsed:.2?}");
    }

    Ok(pieces)
}

fn verify_tr_files(
    piece_slices: &[[u8; SHA1_HASH_SIZE]],
    tr_files: &[TrFile],
    base_path: &Path,
    piece_length: usize,
    n_jobs: usize,
    quiet: bool,
) -> TrResult<FailedInfo> {
    let piece_file_info = calc_piece_file_info(tr_files, piece_length);

    let mut file_status_map: HashMap<String, bool> = HashMap::new();
    let mut failed_info = FailedInfo {
        files: HashSet::new(),
        files_known: HashSet::new(),
        pieces: HashSet::new(),
    };
    let pieces_count = piece_slices.len();

    let pb = if !quiet {
        let pb = ProgressBar::new(pieces_count as u64);
        pb.set_style(
            ProgressStyle::with_template(PB_STYLE_TEMPLATE)
                .unwrap()
                .progress_chars(PB_PROGRESS_CHARS),
        );
        Some(pb)
    } else {
        None
    };

    for (i, piece) in piece_file_info.iter().enumerate() {
        let mut files_ok: bool = true;
        for file_hash_info in piece {
            let tr_file = &tr_files[file_hash_info.file_index];
            let f_path = tr_file.join_full_path(base_path);
            let f_path_str = f_path
                .to_str()
                .ok_or_else(|| TrError::InvalidPath(String::from("Path contains invalid UTF-8")))?
                .to_string();
            match file_status_map.entry(f_path_str) {
                Entry::Vacant(entry) => {
                    let file_ok = metadata(&f_path)
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
            if let Some(ref pb) = pb {
                pb.inc(1);
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
        &pb,
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

    if let Some(ref pb) = pb {
        let elapsed = pb.elapsed();
        pb.finish_and_clear();
        println!("{FINISHED_LINE_PREFIX} [{pieces_count}/{pieces_count}] {FINISHED_LINE_SUFFIX}");
        println!("Processed {pieces_count} pieces in {elapsed:.2?}");
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

thread_local! {
    static FIXED_BUFFER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn hash_piece_file(
    piece_length: usize,
    piece_file_info: &[Vec<FileHashInfo>],
    tr_files: &[TrFile],
    base_path: &Path,
    pb: &Option<ProgressBar>,
    n_jobs: usize,
) -> TrResult<Vec<[u8; SHA1_HASH_SIZE]>> {
    let f_path_list: Vec<_> = tr_files
        .iter()
        .map(|tr_file| tr_file.join_full_path(base_path))
        .collect();

    let results: Result<Vec<[u8; SHA1_HASH_SIZE]>, TrError> = {
        let pool = ThreadPoolBuilder::new()
            .num_threads(n_jobs)
            .build()
            .map_err(|e| TrError::ParseError(format!("Failed to create thread pool: {e}")))?;

        pool.install(|| {
            piece_file_info
                .par_iter()
                .map(|piece| -> TrResult<[u8; SHA1_HASH_SIZE]> {
                    let mut hasher = Sha1::new();

                    FIXED_BUFFER.with(|buf_cell| -> TrResult<()> {
                        let mut buf = buf_cell.borrow_mut();
                        if buf.capacity() < piece_length {
                            buf.resize(piece_length, 0);
                        }

                        for file_hash_info in piece {
                            let f_path = &f_path_list[file_hash_info.file_index];
                            let mut f = File::open(f_path)?;
                            f.seek(SeekFrom::Start(file_hash_info.file_offset as u64))?;

                            let buf_slice = &mut buf[..file_hash_info.length];
                            let n = f.read(buf_slice)?;
                            hasher.update(&buf_slice[..n]);
                        }
                        Ok(())
                    })?;

                    let calc_hash = hasher.finalize();
                    let mut hash_arr = [0u8; SHA1_HASH_SIZE];
                    hash_arr.copy_from_slice(&calc_hash);

                    if let Some(pb) = pb {
                        pb.inc(1);
                    }

                    Ok(hash_arr)
                })
                .collect()
        })
    };

    results
}
