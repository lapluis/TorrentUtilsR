use std::cmp;

use crate::tr_file::TrFile;

pub(crate) struct FileHashInfo {
    pub(crate) file_index: usize,
    pub(crate) file_offset: usize,
    pub(crate) length: usize,
}

pub(crate) fn calc_piece_file_info(
    tr_files: &[TrFile],
    piece_length: usize,
) -> Vec<Vec<FileHashInfo>> {
    let total_size: usize = tr_files.iter().map(|f| f.length()).sum();
    let pieces_count = total_size.div_ceil(piece_length);

    let mut piece_file_info: Vec<Vec<FileHashInfo>> = Vec::with_capacity(pieces_count);
    let mut unfilled_size = 0usize;

    for (file_index, tr_file) in tr_files.iter().enumerate() {
        let mut rest_size = tr_file.length();
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
