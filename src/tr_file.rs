use std::collections::HashMap;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::path::{Path, PathBuf};

use natord::compare_ignore_case;

use crate::bencode::{bencode_string, bencode_string_list, bencode_uint};
use crate::utils::human_size;

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

#[derive(Debug)]
pub struct FileTree {
    name: String,
    length: Option<usize>, // None -> dir，Some(size) -> file
    children: HashMap<String, FileTree>,
}

impl FileTree {
    fn new_dir(name: &str) -> Self {
        FileTree {
            name: name.into(),
            length: None,
            children: HashMap::new(),
        }
    }
    fn new_file(name: &str, size: usize) -> Self {
        FileTree {
            name: name.into(),
            length: Some(size),
            children: HashMap::new(),
        }
    }

    fn insert_path(&mut self, segments: &[String], size: usize) {
        if segments.is_empty() {
            return;
        }
        if segments.len() == 1 {
            self.children
                .entry(segments[0].clone())
                .and_modify(|n| {
                    n.length = Some(size);
                })
                .or_insert_with(|| FileTree::new_file(&segments[0], size));
        } else {
            let dir = self
                .children
                .entry(segments[0].clone())
                .or_insert_with(|| FileTree::new_dir(&segments[0]));
            dir.insert_path(&segments[1..], size);
        }
    }

    pub fn build(files: &[TrFile]) -> FileTree {
        let mut root = FileTree::new_dir("");
        for f in files {
            root.insert_path(&f.path, f.length);
        }
        root
    }

    fn fmt_tree(&self, f: &mut Formatter<'_>) -> FmtResult {
        let mut names: Vec<&String> = self.children.keys().collect();
        names.sort_by(|a, b| compare_ignore_case(a, b));

        for (idx, name) in names.iter().enumerate() {
            let last = idx == names.len() - 1;
            let child = self.children.get(*name).expect("tree child exists");
            child.fmt_branch(f, "", last)?;
        }
        Ok(())
    }

    fn fmt_branch(&self, f: &mut Formatter<'_>, prefix: &str, is_last: bool) -> FmtResult {
        let (connector, child_prefix) = if is_last {
            ("└── ", "    ")
        } else {
            ("├── ", "│   ")
        };

        match self.length {
            Some(sz) => writeln!(
                f,
                "{prefix}{connector}{} ({sz} [{}])",
                self.name,
                human_size(sz)
            )?,
            None => writeln!(f, "{prefix}{connector}{}", self.name)?,
        }

        let mut names: Vec<&String> = self.children.keys().collect();
        names.sort_by(|a, b| compare_ignore_case(a, b));

        let new_prefix = format!("{prefix}{child_prefix}");
        for (idx, name) in names.iter().enumerate() {
            let last = idx == names.len() - 1;
            let child = self.children.get(*name).expect("tree child exists");
            child.fmt_branch(f, &new_prefix, last)?;
        }
        Ok(())
    }
}

impl Display for FileTree {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        self.fmt_tree(f)
    }
}
