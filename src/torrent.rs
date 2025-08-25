use sha1::{Digest, Sha1};
use std::cmp::min;
use std::collections::HashMap;
use std::fs::{File, metadata, read};
use std::io::{Read, Result, Write};
use std::path::{MAIN_SEPARATOR, Path, PathBuf};
use walkdir::WalkDir;

struct TrFile {
    length: u64,
    path: Vec<String>,
}

struct TrInfo {
    files: Option<Vec<TrFile>>,
    length: Option<u64>,
    name: Option<String>,
    piece_length: u64,
    pieces: Vec<u8>,
    private: bool,
}

pub struct Torrent {
    torrent_path: String,
    // target_path: Option<String>,
    // piece_size: u64,
    announce: Option<String>,
    announce_list: Option<Vec<Vec<String>>>,
    comment: Option<String>,
    created_by: Option<String>,
    creation_date: Option<u64>,
    encoding: String,
    hash: String,
    info: TrInfo,
}

fn bencode_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut bcode: Vec<u8> = Vec::new();
    let len = bytes.len();
    bcode.extend(len.to_string().as_bytes());
    bcode.push(b':');
    bcode.extend(bytes);
    bcode
}

fn bencode_string(s: &str) -> Vec<u8> {
    let mut bcode: Vec<u8> = Vec::new();
    let len = s.len();
    bcode.extend(len.to_string().as_bytes());
    bcode.push(b':');
    bcode.extend(s.as_bytes());
    bcode
}

fn bencode_integer(i: u64) -> Vec<u8> {
    let mut bcode: Vec<u8> = Vec::new();
    bcode.push(b'i');
    bcode.extend(i.to_string().as_bytes());
    bcode.push(b'e');
    bcode
}

fn bencode_string_list(list: &Vec<String>) -> Vec<u8> {
    let mut bcode: Vec<u8> = Vec::new();
    bcode.push(b'l');
    for item in list {
        bcode.extend(bencode_string(item));
    }
    bcode.push(b'e');
    bcode
}

fn bencode_file_list(list: &[TrFile]) -> Vec<u8> {
    let mut bcode: Vec<u8> = Vec::new();
    bcode.push(b'l');
    for item in list {
        bcode.extend(item.bencode());
    }
    bcode.push(b'e');
    bcode
}

impl TrFile {
    fn bencode(&self) -> Vec<u8> {
        let mut bcode: Vec<u8> = Vec::new();
        bcode.push(b'd');
        bcode.extend(bencode_string("length"));
        bcode.extend(bencode_integer(self.length));
        bcode.extend(bencode_string("path"));
        bcode.extend(bencode_string_list(&self.path));
        bcode.push(b'e');
        bcode
    }
}

impl TrInfo {
    fn new(target_path: String, piece_size: u64, private: bool) -> TrInfo {
        // get target path name
        let base_path = Path::new(&target_path);
        let name = base_path.file_name().unwrap().to_str().unwrap();
        let mut file_list: Vec<PathBuf> = Vec::new();
        let mut single_file = false;

        // check if target path is file or directory
        let base_metadata = metadata(base_path).unwrap();
        let mut tr_files: Vec<TrFile> = Vec::new();
        if base_metadata.is_file() {
            file_list.push(base_path.to_path_buf());
            single_file = true;
        } else if base_metadata.is_dir() {
            // read directory recursively
            for entry in WalkDir::new(base_path).into_iter().filter_map(|e| e.ok()) {
                if entry.file_type().is_file() {
                    file_list.push(entry.path().to_path_buf());
                    tr_files.push(TrFile {
                        length: metadata(entry.path()).unwrap().len(),
                        path: entry
                            .path()
                            .strip_prefix(base_path)
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .split(MAIN_SEPARATOR)
                            .map(str::to_owned)
                            .collect(),
                    });
                }
            }
        } else {
            panic!("Target path is neither a file nor a directory");
        }

        let chunk_size: usize = 1 << piece_size;
        let mut buf = vec![0u8; 1 << 18]; // 256 KiB buffer
        let mut piece_pos = 0usize;
        let mut pieces = Vec::new();
        let mut piece_count = 0u64;
        let mut hasher = Sha1::new();

        for file_path in &file_list {
            let mut f = File::open(file_path).unwrap();

            loop {
                let n = f.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }

                let mut buf_pos = 0;
                while buf_pos < n {
                    let space = chunk_size - piece_pos;
                    let to_copy = min(space, n - buf_pos);

                    hasher.update(&buf[buf_pos..buf_pos + to_copy]);

                    piece_pos += to_copy;
                    buf_pos += to_copy;

                    if piece_pos == chunk_size {
                        pieces.extend_from_slice(&hasher.finalize_reset());
                        piece_count += 1;
                        piece_pos = 0;
                    }
                }
            }
        }

        if piece_pos > 0 {
            pieces.extend_from_slice(&hasher.finalize());
            piece_count += 1;
        }

        println!("Total pieces: {piece_count}");

        TrInfo {
            files: if !single_file { Some(tr_files) } else { None },
            length: if single_file {
                Some(base_metadata.len())
            } else {
                None
            },
            name: Some(name.to_string()),
            piece_length: chunk_size as u64,
            pieces,
            private,
        }
    }

    fn bencode(&self) -> Vec<u8> {
        let mut bcode: Vec<u8> = Vec::new();
        bcode.push(b'd');
        if self.files.is_some() {
            bcode.extend(bencode_string("files"));
            bcode.extend(bencode_file_list(self.files.as_ref().unwrap()));
        }
        if self.length.is_some() {
            bcode.extend(bencode_string("length"));
            bcode.extend(bencode_integer(self.length.unwrap()));
        }
        if self.name.is_some() {
            bcode.extend(bencode_string("name"));
            bcode.extend(bencode_string(self.name.as_ref().unwrap()));
        }
        bcode.extend(bencode_string("piece length"));
        bcode.extend(bencode_integer(self.piece_length));
        if !self.pieces.is_empty() {
            bcode.extend(bencode_string("pieces"));
            bcode.extend(bencode_bytes(&self.pieces));
        }
        if self.private {
            bcode.extend(bencode_string("private"));
            bcode.extend(bencode_integer(1));
        }
        bcode.push(b'e');
        bcode
    }

    fn hash(&self) -> String {
        let mut hasher = Sha1::new();
        hasher.update(self.bencode());
        let result = hasher.finalize();
        hex::encode(result)
    }
}

pub enum ProcessMode {
    Create,
    Verify,
    // ShowInfo,
}

impl Torrent {
    pub fn new(
        torrent_path: String,
        target_path: Option<String>,
        piece_size: Option<u64>,
        process_mode: ProcessMode,

        announce_list: Option<Vec<String>>,
        comment: Option<String>,
        creation_date: Option<u64>,
        created_by: Option<String>,
        private: bool,
        encoding: String,
    ) -> Self {
        let announce = announce_list.as_ref().map(|v| v[0].clone());

        let piece_size = piece_size.unwrap_or(16);

        let announce_list = if announce_list.is_some() {
            let mut alist: Vec<Vec<String>> = Vec::new();
            for url in announce_list.as_ref().unwrap() {
                alist.push(vec![url.clone()]);
            }
            Some(alist)
        } else {
            None
        };

        let info = match process_mode {
            ProcessMode::Create => {
                if target_path.is_none() {
                    panic!("Target path is required for creating a torrent");
                }
                TrInfo::new(target_path.clone().unwrap(), piece_size, private)
            }
            ProcessMode::Verify => None.unwrap(), // TODO: implement verify mode
        };

        Torrent {
            torrent_path,
            // target_path,
            // piece_size: piece_size,
            announce,
            announce_list,
            comment,
            created_by,
            creation_date,
            encoding,
            hash: info.hash(),
            info,
        }
    }

    pub fn read_torrent(tr_path: String) -> Self {
        let bcode: Vec<u8> = read(tr_path.clone()).expect("failed to read file");
        let bcode_len: usize = bcode.len();

        enum Bencode {
            Int(i64),
            Bytes(Vec<u8>),
            List(Vec<Bencode>),
            Dict(HashMap<String, Bencode>),
        }

        enum Frame {
            List(Vec<Bencode>),
            Dict(Vec<Bencode>),
        }

        fn push_value(stack: &mut [Frame], root: &mut Option<Bencode>, val: Bencode) {
            if let Some(top) = stack.last_mut() {
                match top {
                    Frame::List(items) | Frame::Dict(items) => items.push(val),
                }
            } else if root.is_none() {
                *root = Some(val);
            } else {
                panic!("Malformed input (multiple roots)");
            }
        }

        let mut stack: Vec<Frame> = Vec::new();
        let mut tr_dict: Option<Bencode> = None;
        let mut seek_pos: usize = 0;
        while seek_pos < bcode_len {
            match bcode[seek_pos] {
                b'i' => {
                    if let Some(end) = bcode[seek_pos + 1..].iter().position(|&c| c == b'e') {
                        let j = seek_pos + 1 + end;
                        let num_str = &bcode[seek_pos + 1..j];
                        let num = std::str::from_utf8(num_str)
                            .unwrap()
                            .parse::<i64>()
                            .unwrap();
                        push_value(&mut stack, &mut tr_dict, Bencode::Int(num));
                        seek_pos = j + 1;
                    } else {
                        panic!("Unterminated integer");
                    }
                }
                b'l' => {
                    stack.push(Frame::List(Vec::new()));
                    seek_pos += 1;
                }
                b'd' => {
                    stack.push(Frame::Dict(Vec::new()));
                    seek_pos += 1;
                }
                b'e' => {
                    let frame = stack.pop().expect("Unexpected end");
                    let val = match frame {
                        Frame::List(items) => Bencode::List(items),
                        Frame::Dict(items) => {
                            if items.len() % 2 != 0 {
                                panic!("Malformed dict (odd items)");
                            }
                            let mut map = HashMap::new();
                            let mut it = items.into_iter();
                            while let (Some(k), Some(v)) = (it.next(), it.next()) {
                                match k {
                                    Bencode::Bytes(key) => {
                                        // convert key to str
                                        map.insert(String::from_utf8(key).unwrap(), v);
                                    }
                                    _ => panic!("Dict key must be string"),
                                }
                            }
                            Bencode::Dict(map)
                        }
                    };
                    push_value(&mut stack, &mut tr_dict, val);
                    seek_pos += 1;
                }
                b'0'..=b'9' => {
                    if let Some(colon) = bcode[seek_pos..].iter().position(|&c| c == b':') {
                        let j = seek_pos + colon;
                        let len_str = &bcode[seek_pos..j];
                        let length = std::str::from_utf8(len_str)
                            .unwrap()
                            .parse::<usize>()
                            .unwrap();
                        let start = j + 1;
                        let end = start + length;
                        if end > bcode_len {
                            panic!("Truncated string");
                        }
                        let slice = bcode[start..end].to_vec();
                        push_value(&mut stack, &mut tr_dict, Bencode::Bytes(slice));
                        seek_pos = end;
                    } else {
                        panic!("Malformed string length");
                    }
                }
                _ => panic!("Unknown token"),
            }
        }

        if !stack.is_empty() {
            panic!("Unterminated list/dict");
        }

        let tr_dict = match tr_dict.expect("Empty input") {
            Bencode::Dict(m) => m,
            _ => panic!("Torrent root is not a dictionary"),
        };

        let info_dict = match tr_dict.get(&String::from("info")).expect("Error info dict") {
            Bencode::Dict(m) => m,
            _ => panic!("Torrent info is not a dictionary"),
        };

        let tr_file_list: Vec<TrFile> = match info_dict.get("files") {
            Some(Bencode::List(files)) => {
                let mut tr_files: Vec<TrFile> = Vec::new();
                for file in files {
                    let file = match file {
                        Bencode::Dict(m) => m,
                        _ => panic!("File entry is not a dictionary"),
                    };
                    tr_files.push(TrFile {
                        length: match file.get("length") {
                            Some(Bencode::Int(i)) => *i as u64,
                            _ => panic!("File length is not an integer"),
                        },
                        path: match file.get("path") {
                            Some(Bencode::List(p)) => {
                                let mut path_parts: Vec<String> = Vec::new();
                                for part in p {
                                    match part {
                                        Bencode::Bytes(b) => {
                                            path_parts.push(String::from_utf8(b.clone()).unwrap())
                                        }
                                        _ => panic!("Path part is not a string"),
                                    }
                                }
                                path_parts
                            }
                            _ => panic!("File path is not a list"),
                        },
                    });
                }
                tr_files
            }
            _ => Vec::new(), // Not a multi-file torrent
        };

        let tr_info = TrInfo {
            files: if !tr_file_list.is_empty() {
                Some(tr_file_list)
            } else {
                None
            },
            length: match info_dict.get("length") {
                Some(Bencode::Int(i)) => Some(*i as u64),
                _ => None,
            },
            name: match info_dict.get("name") {
                Some(Bencode::Bytes(b)) => Some(String::from_utf8(b.clone()).unwrap()),
                _ => None,
            },
            piece_length: match info_dict.get("piece length") {
                Some(Bencode::Int(i)) => *i as u64,
                _ => panic!("Piece length is not an integer"),
            },
            pieces: match info_dict.get("pieces") {
                Some(Bencode::Bytes(b)) => b.clone(),
                _ => panic!("Pieces is not a byte string"),
            },
            private: match info_dict.get("private") {
                Some(Bencode::Int(i)) => *i != 0,
                _ => false,
            },
        };

        Torrent {
            torrent_path: tr_path,
            announce: match tr_dict.get("announce") {
                Some(Bencode::Bytes(b)) => Some(String::from_utf8(b.clone()).unwrap()),
                _ => None,
            },
            announce_list: match tr_dict.get("announce-list") {
                Some(Bencode::List(lists)) => {
                    let mut alist: Vec<Vec<String>> = Vec::new();
                    for tier in lists {
                        match tier {
                            Bencode::List(urls) => {
                                let mut tier_list: Vec<String> = Vec::new();
                                for url in urls {
                                    match url {
                                        Bencode::Bytes(b) => {
                                            tier_list.push(String::from_utf8(b.clone()).unwrap())
                                        }
                                        _ => panic!("Announce URL is not a string"),
                                    }
                                }
                                alist.push(tier_list);
                            }
                            _ => panic!("Announce tier is not a list"),
                        }
                    }
                    Some(alist)
                }
                _ => None,
            },
            comment: match tr_dict.get("comment") {
                Some(Bencode::Bytes(b)) => Some(String::from_utf8(b.clone()).unwrap()),
                _ => None,
            },
            created_by: match tr_dict.get("created by") {
                Some(Bencode::Bytes(b)) => Some(String::from_utf8(b.clone()).unwrap()),
                _ => None,
            },
            creation_date: match tr_dict.get("creation date") {
                Some(Bencode::Int(i)) => Some(*i as u64),
                _ => None,
            },
            encoding: match tr_dict.get("encoding") {
                Some(Bencode::Bytes(b)) => String::from_utf8(b.clone()).unwrap(),
                _ => String::new(),
            },
            hash: match tr_dict.get("hash") {
                Some(Bencode::Bytes(b)) => String::from_utf8(b.clone()).unwrap(),
                _ => String::new(),
            },
            info: tr_info,
        }
    }

    fn bencode(&self) -> Vec<u8> {
        let mut bcode: Vec<u8> = Vec::new();
        bcode.push(b'd');
        if self.announce.is_some() {
            bcode.extend(bencode_string("announce"));
            bcode.extend(bencode_string(self.announce.as_ref().unwrap()));
        }
        if self.announce_list.is_some() {
            bcode.extend(bencode_string("announce-list"));
            bcode.push(b'l');
            for tier in self.announce_list.as_ref().unwrap() {
                bcode.push(b'l');
                for url in tier {
                    bcode.extend(bencode_string(url));
                }
                bcode.push(b'e');
            }
            bcode.push(b'e');
        }
        if self.comment.is_some() {
            bcode.extend(bencode_string("comment"));
            bcode.extend(bencode_string(self.comment.as_ref().unwrap()));
        }
        if self.created_by.is_some() {
            bcode.extend(bencode_string("created by"));
            bcode.extend(bencode_string(self.created_by.as_ref().unwrap()));
        }
        if self.creation_date.is_none() {
            bcode.extend(bencode_string("creation date"));
            bcode.extend(bencode_integer(self.creation_date.unwrap()));
        }
        if !self.encoding.is_empty() {
            bcode.extend(bencode_string("encoding"));
            bcode.extend(bencode_string(&self.encoding));
        }
        bcode.extend(bencode_string("info"));
        bcode.extend(self.info.bencode());
        bcode.extend(bencode_string("hash"));
        bcode.extend(bencode_string(&self.hash));
        bcode.push(b'e');
        bcode
    }

    pub fn write_to_file(&self) -> Result<()> {
        let mut file = File::create(&self.torrent_path)?;
        file.write_all(&self.bencode())?;
        Ok(())
    }
}
