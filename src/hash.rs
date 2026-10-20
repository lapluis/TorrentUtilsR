use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use rayon::{ThreadPoolBuilder, prelude::*};
use sha1::{Digest, Sha1};

use crate::piece::{FileHashInfo, calc_piece_file_info};
use crate::progress::ProgressReporter;
use crate::tr_file::TrFile;
use crate::utils::{TrError, TrResult};

pub(crate) const SHA1_HASH_SIZE: usize = 20;

pub(crate) fn hash_tr_files(
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

pub(crate) fn hash_piece_file(
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

pub(crate) fn split_hash_pieces(piece: &[u8]) -> Vec<[u8; SHA1_HASH_SIZE]> {
    let layer_count = piece.len() / SHA1_HASH_SIZE;
    let mut slices: Vec<[u8; SHA1_HASH_SIZE]> = vec![[0u8; SHA1_HASH_SIZE]; layer_count];
    for i in 0..layer_count {
        slices[i].copy_from_slice(&piece[i * SHA1_HASH_SIZE..(i + 1) * SHA1_HASH_SIZE]);
    }
    slices
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
