use natlex_sort::nat_lex_cmp_ignore;
use std::collections::HashMap;

use crate::bencode::{bencode_string, bencode_string_list, bencode_uint};
use crate::utils::human_size;

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
pub struct Node {
    name: String,
    length: Option<usize>, // None -> dir，Some(size) -> file
    children: HashMap<String, Node>,
}

impl Node {
    fn new_dir(name: &str) -> Self {
        Node {
            name: name.into(),
            length: None,
            children: HashMap::new(),
        }
    }
    fn new_file(name: &str, size: usize) -> Self {
        Node {
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
                .or_insert_with(|| Node::new_file(&segments[0], size));
        } else {
            let dir = self
                .children
                .entry(segments[0].clone())
                .or_insert_with(|| Node::new_dir(&segments[0]));
            dir.insert_path(&segments[1..], size);
        }
    }

    pub fn build_tree(files: &[TrFile]) -> Node {
        let mut root = Node::new_dir("");
        for f in files {
            root.insert_path(&f.path, f.length);
        }
        root
    }

    pub fn print_tree(&self) {
        let mut names: Vec<&String> = self.children.keys().collect();
        names.sort_by(|a, b| nat_lex_cmp_ignore(a, b));

        for (idx, name) in names.iter().enumerate() {
            let last = idx == names.len() - 1;
            let child = self.children.get(*name).unwrap();
            child.print_branch("", last);
        }
    }

    fn print_branch(&self, prefix: &str, is_last: bool) {
        let (connector, child_prefix) = if is_last {
            ("└── ", "    ")
        } else {
            ("├── ", "│   ")
        };

        match self.length {
            Some(sz) => println!(
                "{}{}{} ({} [{}])",
                prefix,
                connector,
                self.name,
                sz,
                human_size(sz)
            ),
            None => println!("{}{}{}", prefix, connector, self.name),
        }

        let mut names: Vec<&String> = self.children.keys().collect();
        names.sort_by(|a, b| nat_lex_cmp_ignore(a, b));

        let new_prefix = format!("{}{}", prefix, child_prefix);
        for (idx, name) in names.iter().enumerate() {
            let last = idx == names.len() - 1;
            let child = self.children.get(*name).unwrap();
            child.print_branch(&new_prefix, last);
        }
    }
}
