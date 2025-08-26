use chrono::{TimeZone, Local};
use sha1::{Digest, Sha1};
use std::cmp;
use std::collections::{HashMap, HashSet};
use std::fs::{File, metadata, read};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{MAIN_SEPARATOR, Path, PathBuf};
use std::{fmt, vec};
use walkdir::WalkDir;

struct TrFile {
    length: usize,
    path: Vec<String>,
}

pub struct TrInfo {
    files: Option<Vec<TrFile>>,
    length: Option<usize>,
    name: Option<String>,
    piece_length: usize,
    pieces: Vec<u8>,
    private: bool,
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

fn bencode_uint(i: usize) -> Vec<u8> {
    let mut bcode: Vec<u8> = Vec::new();
    bcode.push(b'i');
    bcode.extend(i.to_string().as_bytes());
    bcode.push(b'e');
    bcode
}

fn bencode_int(i: i64) -> Vec<u8> {
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

fn hash_pieces(base_path: &Path, tr_files: &[TrFile], chunk_size: usize) -> Vec<u8> {
    let mut buf = vec![0u8; 1 << 18]; // 256 KiB buffer
    let mut piece_pos = 0usize;
    let mut pieces = Vec::new();
    let mut piece_count = 0u64;
    let mut hasher = Sha1::new();

    for tr_file in tr_files {
        let f_path = if tr_file.path.is_empty() {
            base_path.to_path_buf()
        } else {
            base_path.join(tr_file.path.iter().collect::<PathBuf>())
        };
        let mut f: File = File::open(f_path).unwrap();

        loop {
            let n = f.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }

            let mut buf_pos = 0;
            while buf_pos < n {
                let space = chunk_size - piece_pos;
                let to_copy = cmp::min(space, n - buf_pos);

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

    #[cfg(debug_assertions)]
    {
        println!("Total pieces: {piece_count}");
    }

    pieces
}

fn split_hash_pieces(piece: &[u8]) -> Vec<[u8; 20]> {
    let layer_count = piece.len() / 20;
    let mut folder: Vec<[u8; 20]> = vec![[0u8; 20]; layer_count];
    for i in 0..layer_count {
        folder[i].copy_from_slice(&piece[i * 20..(i + 1) * 20]);
    }
    folder
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

impl TrInfo {
    fn new(target_path: String, piece_size: u64, private: bool) -> TrInfo {
        let base_path = Path::new(&target_path);
        let name = base_path.file_name().unwrap().to_str().unwrap();
        let mut single_file = false;

        let base_metadata = metadata(base_path).unwrap();
        let mut tr_files: Vec<TrFile> = Vec::new();

        if base_metadata.is_file() {
            single_file = true;
            tr_files.push(TrFile {
                length: base_metadata.len() as usize,
                path: Vec::new(),
            });
        } else if base_metadata.is_dir() {
            for entry in WalkDir::new(base_path).into_iter().filter_map(|e| e.ok()) {
                if entry.file_type().is_file() {
                    tr_files.push(TrFile {
                        length: metadata(entry.path()).unwrap().len() as usize,
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
        let pieces = hash_pieces(base_path, &tr_files, chunk_size);

        TrInfo {
            files: if !single_file { Some(tr_files) } else { None },
            length: if single_file {
                Some(base_metadata.len() as usize)
            } else {
                None
            },
            name: Some(name.to_string()),
            piece_length: chunk_size,
            pieces,
            private,
        }
    }

    pub fn verify(&self, target_path: String) {
        let base_path = Path::new(&target_path);
        let tr_files = match self.files {
            Some(ref files) => files,
            None => &vec![TrFile {
                length: self.length.unwrap(),
                path: Vec::new(),
            }],
        };

        let mut piece_file_info: Vec<Vec<(usize, usize, usize)>> = Vec::new(); // piece_index -> [(file_index, file_offset, length), ...]
        let mut file_offset = 0usize;
        let mut pool_size = 0usize;

        for (file_index, tr_file) in tr_files.iter().enumerate() {
            pool_size += tr_file.length;
            if file_offset > 0 {
                if let Some(last) = piece_file_info.last_mut() {
                    if pool_size > self.piece_length {
                        last.push((file_index, 0, self.piece_length - file_offset));
                    } else if pool_size < self.piece_length {
                        last.push((file_index, 0, tr_file.length));
                        file_offset += tr_file.length;
                        continue;
                    } else {
                        last.push((file_index, 0, tr_file.length));
                        file_offset = 0;
                        pool_size = 0;
                        continue;
                    }
                }
            }
            let piece_count =
                (pool_size + file_offset) / self.piece_length - if file_offset > 0 { 1 } else { 0 };
            let start_pos = (self.piece_length - file_offset) % self.piece_length;
            for i in 0..piece_count {
                piece_file_info.push(vec![(
                    file_index,
                    start_pos + self.piece_length * i,
                    self.piece_length,
                )]);
            }
            file_offset = pool_size % self.piece_length;
            if file_offset > 0 {
                piece_file_info.push(vec![(
                    file_index,
                    start_pos + self.piece_length * piece_count,
                    file_offset,
                )]);
                pool_size = file_offset;
            } else {
                pool_size = 0;
            }
        }

        let piece_slices: Vec<[u8; 20]> = split_hash_pieces(&self.pieces);
        let mut file_status_map: HashMap<String, bool> = HashMap::new();
        let mut failed_files: HashSet<usize> = HashSet::new();
        let mut failed_files_know: HashSet<usize> = HashSet::new();
        let mut failed_pieces: HashSet<usize> = HashSet::new();

        let mut hasher = Sha1::new();

        for (i, piece_hash) in piece_slices.iter().enumerate() {
            let mut files_ok: bool = true;
            for (file_index, _, _) in &piece_file_info[i] {
                let tr_file = &tr_files[*file_index];
                let f_path = if tr_file.path.is_empty() {
                    base_path.to_path_buf()
                } else {
                    base_path.join(tr_file.path.iter().collect::<PathBuf>())
                };
                let f_path_str = f_path.to_str().unwrap().to_string();
                if !file_status_map.contains_key(&f_path_str) {
                    let f_meta = metadata(&f_path);
                    if f_meta.is_err() || f_meta.unwrap().len() != tr_file.length as u64 {
                        file_status_map.insert(f_path_str.clone(), false);
                        failed_files_know.insert(*file_index);
                        files_ok = false;
                    } else {
                        file_status_map.insert(f_path_str.clone(), true);
                    }
                }
            }
            if !files_ok {
                failed_pieces.insert(i);
                for (file_index, _, _) in &piece_file_info[i] {
                    failed_files.insert(*file_index);
                }
                continue;
            }
            for (file_index, file_offset, length) in &piece_file_info[i] {
                let tr_file = &tr_files[*file_index];
                let f_path = if tr_file.path.is_empty() {
                    base_path.to_path_buf()
                } else {
                    base_path.join(tr_file.path.iter().collect::<PathBuf>())
                };
                let mut f: File = File::open(f_path).unwrap();
                f.seek(SeekFrom::Start(*file_offset as u64)).unwrap();
                let mut buf = vec![0u8; *length];
                let n = f.read(&mut buf).unwrap();
                if n != *length {
                    buf.truncate(n);
                }
                hasher.update(&buf);
            }
            let calc_hash = hasher.finalize_reset();
            if &calc_hash[..] != piece_hash {
                files_ok = false;
            }
            if !files_ok {
                failed_pieces.insert(i);
                for (file_index, _, _) in &piece_file_info[i] {
                    failed_files.insert(*file_index);
                }
            }
        }

        println!("Verification Result:");
        if failed_files.is_empty() {
            println!("All files are OK.");
        } else {
            println!("Some files failed verification:");
            let mut failed_files: Vec<usize> = failed_files.iter().cloned().collect();
            failed_files.sort();
            for file_index in failed_files {
                let tr_file = &tr_files[file_index];
                let rel_path = if tr_file.path.is_empty() {
                    self.name.as_ref().unwrap().to_string()
                } else {
                    tr_file.path.join("/")
                };
                let known_issue = if failed_files_know.contains(&file_index) {
                    " [missing or size mismatch]"
                } else {
                    ""
                };
                println!("- {} ({} bytes){}", rel_path, tr_file.length, known_issue);
            }
            println!("\nFailed pieces:");
            let mut failed_pieces: Vec<usize> = failed_pieces.iter().cloned().collect();
            failed_pieces.sort();
            for piece_index in &failed_pieces {
                println!("- Piece {piece_index}");
            }
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

    fn hash(&self) -> String {
        let mut hasher = Sha1::new();
        hasher.update(self.bencode());
        let result = hasher.finalize();
        hex::encode(result)
    }
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

    pub fn create_torrent(&mut self, target_path: String, piece_size: u64, private: bool) {
        let info = TrInfo::new(target_path, piece_size, private);
        self.hash = Some(info.hash());
        self.info = Some(info);
    }

    pub fn write_to_file(&self, torrent_path: String, force: bool) -> std::io::Result<()> {
        if !force && Path::new(&torrent_path).exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "File already exists",
            ));
        }
        let mut file = File::create(torrent_path)?;
        file.write_all(&self.bencode())?;
        Ok(())
    }

    pub fn read_torrent(tr_path: String) -> Result<Self, String> {
        enum Bencode<'a> {
            Int(usize),
            UInt(i64),
            Bytes(&'a [u8]),
            List(Vec<Bencode<'a>>),
            Dict(HashMap<String, Bencode<'a>>),
        }

        let bcode = read(&tr_path).map_err(|e| format!("failed to read file: {e}"))?;
        let mut pos = 0;

        fn parse_bencode<'a>(data: &'a [u8], pos: &mut usize) -> Result<Bencode<'a>, String> {
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
                            Bencode::Bytes(b) => {
                                String::from_utf8(b.to_vec()).map_err(|_| "invalid utf8 key")?
                            }
                            _ => return Err("dict key not string".into()),
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
                        return Err("truncated string length".into());
                    }
                    let len_str = std::str::from_utf8(&data[start..*pos])
                        .map_err(|_| "invalid utf8 length")?;
                    let len = len_str.parse::<usize>().map_err(|_| "bad string length")?;
                    *pos += 1;
                    let end = *pos + len;
                    if end > data.len() {
                        return Err("truncated string".into());
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
            _ => return Err("torrent root is not a dictionary".into()),
        };

        let info_dict = match tr_dict.get("info") {
            Some(Bencode::Dict(m)) => m,
            _ => return Err("missing info dict".into()),
        };

        let tr_files = match info_dict.get("files") {
            Some(Bencode::List(files)) => {
                let mut out = Vec::new();
                for file in files {
                    if let Bencode::Dict(m) = file {
                        let length = match m.get("length") {
                            Some(Bencode::Int(i)) => *i,
                            _ => return Err("file length invalid".into()),
                        };
                        let path = match m.get("path") {
                            Some(Bencode::List(parts)) => {
                                let mut ps = Vec::new();
                                for part in parts {
                                    if let Bencode::Bytes(b) = part {
                                        ps.push(String::from_utf8(b.to_vec()).unwrap());
                                    }
                                }
                                ps
                            }
                            _ => return Err("file path invalid".into()),
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
                Some(Bencode::Bytes(b)) => Some(String::from_utf8(b.to_vec()).unwrap()),
                _ => None,
            },
            piece_length: match info_dict.get("piece length") {
                Some(Bencode::Int(i)) => *i,
                _ => return Err("piece length missing".into()),
            },
            pieces: match info_dict.get("pieces") {
                Some(Bencode::Bytes(b)) => b.to_vec(),
                _ => return Err("pieces missing".into()),
            },
            private: match info_dict.get("private") {
                Some(Bencode::Int(i)) => *i != 0,
                _ => false,
            },
        };

        Ok(Torrent {
            announce: match tr_dict.get("announce") {
                Some(Bencode::Bytes(b)) => Some(String::from_utf8(b.to_vec()).unwrap()),
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
                                            tier_list.push(String::from_utf8(b.to_vec()).unwrap())
                                        }
                                        _ => return Err("Announce URL is not a string".into()),
                                    }
                                }
                                alist.push(tier_list);
                            }
                            _ => return Err("Announce tier is not a list".into()),
                        }
                    }
                    Some(alist)
                }
                _ => None,
            },
            comment: match tr_dict.get("comment") {
                Some(Bencode::Bytes(b)) => Some(String::from_utf8(b.to_vec()).unwrap()),
                _ => None,
            },
            created_by: match tr_dict.get("created by") {
                Some(Bencode::Bytes(b)) => Some(String::from_utf8(b.to_vec()).unwrap()),
                _ => None,
            },
            creation_date: match tr_dict.get("creation date") {
                Some(Bencode::UInt(i)) => Some(*i),
                Some(Bencode::Int(i)) => Some(*i as i64),
                _ => None,
            },
            encoding: match tr_dict.get("encoding") {
                Some(Bencode::Bytes(b)) => Some(String::from_utf8(b.to_vec()).unwrap()),
                _ => None,
            },
            hash: match tr_dict.get("hash") {
                Some(Bencode::Bytes(b)) => Some(String::from_utf8(b.to_vec()).unwrap()),
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
            panic!("info dict is missing");
        }
        if self.hash.is_some() {
            bcode.extend(bencode_string("hash"));
            bcode.extend(bencode_string(self.hash.as_ref().unwrap()));
        }
        bcode.push(b'e');
        bcode
    }
}

impl fmt::Display for Torrent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let info = self.info.as_ref().unwrap();

        writeln!(f, "Torrent Info:")?;
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
                    if shown < 20 {
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
                writeln!(f, "    Truncated at 20 announces...")?;
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
            "  Piece length: {piece_length}",
            piece_length = info.piece_length
        )?;
        writeln!(f, "  Private: {private}", private = info.private)?;
        if let Some(files) = &info.files {
            writeln!(f, "  Files (RelPath [Length]):")?;
            let mut shown = 0;
            let mut truncated = false;
            for file in files {
                if shown < 100 {
                    let path_str = file.path.join("/");
                    writeln!(f, "    - {path_str} [{length} bytes]", length = file.length)?;
                    shown += 1;
                } else {
                    truncated = true;
                    break;
                }
            }
            if truncated {
                writeln!(f, "    Truncated at 100 files...")?;
            }
        } else if let Some(length) = info.length {
            writeln!(f, "  Length: {length}")?;
        }
        Ok(())
    }
}
