use std::collections::HashMap;
use std::fmt::{Display, Formatter, Result as fmtResult};
use std::fs::{File, read};
use std::io::{Error as ioError, ErrorKind, Result as ioResult, Write, stdout};
use std::path::Path;

use chrono::{Local, TimeZone};

use crate::bencode::{bencode_int, bencode_string};
use crate::tr_file::{Node, TrFile};
use crate::tr_info::TrInfo;
use crate::utils::{TrError, TrResult, human_size};

const MAX_DISPLAYED_ANNOUNCES: usize = 20;
const MAX_DISPLAYED_FILES: usize = 100;

pub enum WalkMode {
    Default,
    Alphabetical,
    BreadthFirstAlphabetical, // tu like
    BreadthFirstLevel,        // qb like
    FileSize,
}

pub struct Torrent {
    announce: Option<String>,
    announce_list: Option<Vec<Vec<String>>>,
    comment: Option<String>,
    created_by: Option<String>,
    creation_date: Option<i64>,
    encoding: Option<String>,
    hash: Option<String>,
    info: Option<TrInfo>,
}

impl Torrent {
    pub fn new(
        announce: Option<String>,
        announce_list: Option<Vec<Vec<String>>>,
        comment: Option<String>,
        created_by: Option<String>,
        creation_date: Option<i64>,
        encoding: Option<String>,
    ) -> Self {
        Torrent {
            announce,
            announce_list,
            comment,
            created_by,
            creation_date,
            encoding,
            hash: None,
            info: None,
        }
    }

    pub fn create_torrent(
        &mut self,
        target_path: String,
        piece_length: usize,
        private: bool,
        n_jobs: usize,
        quiet: bool,
        walk_mode: WalkMode,
    ) -> TrResult<()> {
        let info = TrInfo::new(target_path, piece_length, private, n_jobs, quiet, walk_mode)?;
        self.hash = Some(info.hash());
        self.info = Some(info);
        Ok(())
    }

    pub fn write_to_file(&self, torrent_path: String, force: bool) -> ioResult<()> {
        if !force && Path::new(&torrent_path).exists() {
            return Err(ioError::new(
                ErrorKind::AlreadyExists,
                "File already exists, use -f to overwrite",
            ));
        }
        let mut file = File::create(torrent_path)?;
        file.write_all(&self.bencode())?;
        Ok(())
    }

    pub fn read_torrent(tr_path: String) -> TrResult<Self> {
        enum Bencode<'a> {
            Int(usize),
            UInt(i64),
            Bytes(&'a [u8]),
            List(Vec<Bencode<'a>>),
            Dict(HashMap<String, Bencode<'a>>),
        }

        let bcode = read(&tr_path)?;
        let mut pos = 0;

        fn parse_bencode<'a>(data: &'a [u8], pos: &mut usize) -> TrResult<Bencode<'a>> {
            match data.get(*pos) {
                Some(b'i') => {
                    *pos += 1;
                    let start = *pos;
                    while *pos < data.len() && data[*pos] != b'e' {
                        *pos += 1;
                    }
                    if *pos >= data.len() {
                        return Err("unterminated integer".into());
                    }
                    let num_str = std::str::from_utf8(&data[start..*pos])
                        .map_err(|_| "invalid utf8 in int")?;
                    *pos += 1;
                    if num_str.starts_with("-") {
                        let val = num_str.parse::<i64>().map_err(|_| "invalid int")?;
                        Ok(Bencode::UInt(val))
                    } else {
                        let val = num_str.parse::<usize>().map_err(|_| "invalid int")?;
                        Ok(Bencode::Int(val))
                    }
                }
                Some(b'l') => {
                    *pos += 1;
                    let mut items = Vec::new();
                    while data.get(*pos) != Some(&b'e') {
                        items.push(parse_bencode(data, pos)?);
                    }
                    *pos += 1;
                    Ok(Bencode::List(items))
                }
                Some(b'd') => {
                    *pos += 1;
                    let mut map = HashMap::new();
                    while data.get(*pos) != Some(&b'e') {
                        let key = match parse_bencode(data, pos)? {
                            Bencode::Bytes(b) => String::from_utf8(b.to_vec()).map_err(|_| {
                                TrError::InvalidTorrent("invalid utf8 key".to_string())
                            })?,
                            _ => {
                                return Err(TrError::InvalidTorrent(
                                    "dict key not string".to_string(),
                                ));
                            }
                        };
                        let val = parse_bencode(data, pos)?;
                        map.insert(key, val);
                    }
                    *pos += 1;
                    Ok(Bencode::Dict(map))
                }
                Some(b'0'..=b'9') => {
                    let start = *pos;
                    while *pos < data.len() && data[*pos] != b':' {
                        *pos += 1;
                    }
                    if *pos >= data.len() {
                        return Err(TrError::InvalidTorrent(
                            "truncated string length".to_string(),
                        ));
                    }
                    let len_str = std::str::from_utf8(&data[start..*pos])
                        .map_err(|_| "invalid utf8 length")?;
                    let len = len_str.parse::<usize>().map_err(|_| "bad string length")?;
                    *pos += 1;
                    let end = *pos + len;
                    if end > data.len() {
                        return Err(TrError::InvalidTorrent("truncated string".to_string()));
                    }
                    let slice = &data[*pos..end];
                    *pos = end;
                    Ok(Bencode::Bytes(slice))
                }
                Some(_) => Err("unknown token".into()),
                None => Err("unexpected EOF".into()),
            }
        }

        let root = parse_bencode(&bcode, &mut pos)?;
        let tr_dict = match root {
            Bencode::Dict(m) => m,
            _ => {
                return Err(TrError::InvalidTorrent(
                    "torrent root is not a dictionary".to_string(),
                ));
            }
        };

        let info_dict = match tr_dict.get("info") {
            Some(Bencode::Dict(m)) => m,
            _ => {
                return Err(TrError::InvalidTorrent("missing info dict".to_string()));
            }
        };

        let tr_files = match info_dict.get("files") {
            Some(Bencode::List(files)) => {
                let mut out = Vec::new();
                for file in files {
                    if let Bencode::Dict(m) = file {
                        let length = match m.get("length") {
                            Some(Bencode::Int(i)) => *i,
                            _ => {
                                return Err(TrError::InvalidTorrent(
                                    "file length invalid".to_string(),
                                ));
                            }
                        };
                        let path = match m.get("path") {
                            Some(Bencode::List(parts)) => {
                                let mut ps = Vec::new();
                                for part in parts {
                                    if let Bencode::Bytes(b) = part {
                                        ps.push(String::from_utf8(b.to_vec())?);
                                    }
                                }
                                ps
                            }
                            _ => {
                                return Err(TrError::InvalidTorrent(
                                    "file path invalid".to_string(),
                                ));
                            }
                        };
                        out.push(TrFile { length, path });
                    }
                }
                Some(out)
            }
            _ => None,
        };

        let tr_info = TrInfo {
            files: tr_files,
            length: match info_dict.get("length") {
                Some(Bencode::Int(i)) => Some(*i),
                _ => None,
            },
            name: match info_dict.get("name") {
                Some(Bencode::Bytes(b)) => Some(String::from_utf8(b.to_vec())?),
                _ => None,
            },
            piece_length: match info_dict.get("piece length") {
                Some(Bencode::Int(i)) => *i,
                _ => {
                    return Err(TrError::InvalidTorrent("piece length missing".to_string()));
                }
            },
            pieces: match info_dict.get("pieces") {
                Some(Bencode::Bytes(b)) => b.to_vec(),
                _ => return Err(TrError::InvalidTorrent("pieces missing".to_string())),
            },
            private: match info_dict.get("private") {
                Some(Bencode::Int(i)) => *i != 0,
                _ => false,
            },
        };

        Ok(Torrent {
            announce: match tr_dict.get("announce") {
                Some(Bencode::Bytes(b)) => Some(String::from_utf8(b.to_vec())?),
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
                                            tier_list.push(String::from_utf8(b.to_vec())?)
                                        }
                                        _ => {
                                            return Err(TrError::InvalidTorrent(
                                                "Announce URL is not a string".to_string(),
                                            ));
                                        }
                                    }
                                }
                                alist.push(tier_list);
                            }
                            _ => {
                                return Err(TrError::InvalidTorrent(
                                    "Announce tier is not a list".to_string(),
                                ));
                            }
                        }
                    }
                    Some(alist)
                }
                _ => None,
            },
            comment: match tr_dict.get("comment") {
                Some(Bencode::Bytes(b)) => Some(String::from_utf8(b.to_vec())?),
                _ => None,
            },
            created_by: match tr_dict.get("created by") {
                Some(Bencode::Bytes(b)) => Some(String::from_utf8(b.to_vec())?),
                _ => None,
            },
            creation_date: match tr_dict.get("creation date") {
                Some(Bencode::UInt(i)) => Some(*i),
                Some(Bencode::Int(i)) => Some(*i as i64),
                _ => None,
            },
            encoding: match tr_dict.get("encoding") {
                Some(Bencode::Bytes(b)) => Some(String::from_utf8(b.to_vec())?),
                _ => None,
            },
            hash: match tr_dict.get("hash") {
                Some(Bencode::Bytes(b)) => Some(String::from_utf8(b.to_vec())?),
                _ => None,
            },
            info: Some(tr_info),
        })
    }

    pub fn get_info(&self) -> Option<&TrInfo> {
        self.info.as_ref()
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
        if self.creation_date.is_some() {
            bcode.extend(bencode_string("creation date"));
            bcode.extend(bencode_int(self.creation_date.unwrap()));
        }
        if self.encoding.is_some() {
            bcode.extend(bencode_string("encoding"));
            bcode.extend(bencode_string(self.encoding.as_ref().unwrap()));
        }
        if self.info.is_some() {
            bcode.extend(bencode_string("info"));
            bcode.extend(self.info.as_ref().unwrap().bencode());
        } else {
            eprintln!("Warning: info dict is missing, creating empty bencode");
        }
        if self.hash.is_some() {
            bcode.extend(bencode_string("hash"));
            bcode.extend(bencode_string(self.hash.as_ref().unwrap()));
        }
        bcode.push(b'e');
        bcode
    }

    pub fn print_file_tree(&self) {
        match &self.info {
            Some(info) => {
                if let Some(name) = &info.name {
                    println!("{name}");
                }
                let _ = stdout().flush();
                if let Some(files) = &info.files {
                    let file_tree = Node::build_tree(files);
                    file_tree.print_tree();
                } else if let Some(length) = info.length {
                    println!("  [Single file, {} bytes ({})]", length, human_size(length));
                } else {
                    println!("  [No files information available]");
                }
            }
            None => {
                println!("[No torrent info available]");
            }
        }
    }
}

impl Display for Torrent {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmtResult {
        writeln!(f, "Torrent Info:")?;

        match &self.info {
            Some(info) => {
                if let Some(name) = &info.name {
                    writeln!(f, "  Name: {name}")?;
                }

                if let Some(announce_list) = &self.announce_list {
                    writeln!(f, "  Announce List:")?;
                    let pad = if announce_list.len() >= 10 { 2 } else { 1 };
                    let mut shown = 0;
                    let mut truncated = false;
                    for (tier_id, tier) in announce_list.iter().enumerate() {
                        let tier_str = if pad == 2 {
                            format!("{tier_id:02}")
                        } else {
                            format!("{tier_id}")
                        };
                        for url in tier {
                            if shown < MAX_DISPLAYED_ANNOUNCES {
                                writeln!(f, "    Tier {tier_str}: {url}")?;
                                shown += 1;
                            } else {
                                truncated = true;
                                break;
                            }
                        }
                        if truncated {
                            break;
                        }
                    }
                    if truncated {
                        writeln!(f, "    Truncated at {MAX_DISPLAYED_ANNOUNCES} announces...")?;
                    }
                }

                if let Some(comment) = &self.comment {
                    writeln!(f, "  Comment: {comment}")?;
                }
                if let Some(created_by) = &self.created_by {
                    writeln!(f, "  Created by: {created_by}")?;
                }
                if let Some(date) = self.creation_date {
                    let dt = Local
                        .timestamp_opt(date, 0)
                        .unwrap()
                        .format("%Y-%m-%d %H:%M:%S");
                    writeln!(f, "  Creation date: {date} [{dt}]")?;
                }
                if let Some(encoding) = &self.encoding {
                    writeln!(f, "  Encoding: {encoding}")?;
                }
                if let Some(hash) = &self.hash {
                    writeln!(f, "  Hash: {hash}")?;
                }

                writeln!(
                    f,
                    "  Piece length: {} [{}]",
                    info.piece_length,
                    human_size(info.piece_length)
                )?;
                writeln!(f, "  Private: {}", info.private)?;

                if let Some(files) = &info.files {
                    writeln!(f, "  Files (RelPath [Length]):")?;
                    let mut shown = 0;
                    let mut truncated = false;
                    for file in files {
                        if shown < MAX_DISPLAYED_FILES {
                            let path_str = file.path.join("/");
                            writeln!(
                                f,
                                "    - {path_str} [{} bytes ({})]",
                                file.length,
                                human_size(file.length)
                            )?;
                            shown += 1;
                        } else {
                            truncated = true;
                            break;
                        }
                    }
                    if truncated {
                        writeln!(f, "    Truncated at {MAX_DISPLAYED_FILES} files...")?;
                    }
                } else if let Some(length) = info.length {
                    writeln!(f, "  Length: {length}")?;
                }
            }
            None => {
                writeln!(f, "  [No torrent info available]")?;
            }
        }

        Ok(())
    }
}
