use std::path::{Path, PathBuf};

use crate::bencode::{bencode_string, bencode_string_list, bencode_uint};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrFile {
    pub length: usize,
    pub path: Vec<String>,
}

impl TrFile {
    fn bencode(&self) -> Vec<u8> {
        let mut bcode: Vec<u8> = Vec::new();
        bcode.push(b'd');
        bcode.extend(bencode_string("length"));
        bcode.extend(bencode_uint(self.length));
        bcode.extend(bencode_string("path"));
        bcode.extend(bencode_string_list(&self.path));
        bcode.push(b'e');
        bcode
    }

    pub fn join_full_path(&self, base_path: &Path) -> PathBuf {
        let mut full_path = base_path.to_path_buf();
        for segment in &self.path {
            full_path.push(segment);
        }
        full_path
    }
}

pub fn bencode_file_list(list: &[TrFile]) -> Vec<u8> {
    let mut bcode: Vec<u8> = Vec::new();
    bcode.push(b'l');
    for item in list {
        bcode.extend(item.bencode());
    }
    bcode.push(b'e');
    bcode
}
