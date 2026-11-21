use std::path::{Path, PathBuf};

use crate::codec::{write_bencode_string, write_bencode_string_list, write_bencode_uint};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrFile {
    length: usize,
    path: Vec<String>,
}

impl TrFile {
    /// Creates a file entry. Path safety is validated by [`crate::TrInfo::from_parts`].
    pub fn new(length: usize, path: Vec<String>) -> Self {
        Self { length, path }
    }

    pub fn length(&self) -> usize {
        self.length
    }

    pub fn path(&self) -> &[String] {
        &self.path
    }

    fn write_bencode(&self, out: &mut Vec<u8>) {
        out.push(b'd');
        write_bencode_string(out, "length");
        write_bencode_uint(out, self.length);
        write_bencode_string(out, "path");
        write_bencode_string_list(out, &self.path);
        out.push(b'e');
    }

    pub fn join_full_path(&self, base_path: &Path) -> PathBuf {
        let mut full_path = base_path.to_path_buf();
        for segment in &self.path {
            full_path.push(segment);
        }
        full_path
    }
}

pub(crate) fn write_bencode_file_list(out: &mut Vec<u8>, list: &[TrFile]) {
    out.push(b'l');
    for item in list {
        item.write_bencode(out);
    }
    out.push(b'e');
}
