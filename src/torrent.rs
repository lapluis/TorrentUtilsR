use hex;
use sha1::{Digest, Sha1};
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::{fs, io, path};
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
        bcode.extend(bencode_string(&item));
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
        let base_path = path::Path::new(&target_path);
        let name = base_path.file_name().unwrap().to_str().unwrap();
        let mut file_list: Vec<String> = Vec::new();
        let mut single_file = false;

        // check if target path is file or directory
        let metadata = fs::metadata(base_path).unwrap();
        if metadata.is_file() {
            let rel_path = base_path.strip_prefix(base_path).unwrap();
            file_list.push(rel_path.to_str().unwrap().to_string());
            single_file = true;
        } else if metadata.is_dir() {
            // read directory recursively
            for entry in WalkDir::new(base_path).into_iter().filter_map(|e| e.ok()) {
                let path = entry.path();
                let rel_path = path.strip_prefix(base_path).unwrap();
                if path.is_file() {
                    file_list.push(rel_path.to_str().unwrap().to_string());
                }
            }
        } else {
            panic!("Target path is neither a file nor a directory");
        }

        let chunk_size = 2u64 << (piece_size - 1);
        let mut buffer_length: u64 = 0;
        let mut pieces: Vec<u8> = Vec::new();
        let mut piece_count: u64 = 0;
        let mut piece_bytes: Vec<u8> = Vec::new();
        let mut tr_files: Vec<TrFile> = Vec::new();
        for file in &file_list {
            let file_path = if single_file {
                base_path.to_path_buf()
            } else {
                base_path.join(file)
            };
            let file_metadata = fs::metadata(&file_path).unwrap();
            let file_length = file_metadata.len();
            let tr_file_path: Vec<String> = file
                .split(path::MAIN_SEPARATOR)
                .map(|s| s.to_string())
                .collect();
            tr_files.push(TrFile {
                length: file_length,
                path: tr_file_path,
            });

            let f = File::open(&file_path).unwrap();
            let mut reader = BufReader::new(f);
            loop {
                match Self::read_chunk(&mut reader, (chunk_size - buffer_length) as usize) {
                    Ok(Some(chunk)) => {
                        piece_bytes.extend(&chunk);
                        if (piece_bytes.len() as u64) == chunk_size {
                            // calculate SHA1 hash of piece_bytes
                            let mut hasher = Sha1::new();
                            hasher.update(&piece_bytes);
                            let result = hasher.finalize();
                            pieces.extend(&result);
                            piece_bytes.clear();
                            piece_count += 1u64;
                        } else if (piece_bytes.len() as u64) < chunk_size {
                            buffer_length = piece_bytes.len() as u64;
                            continue;
                        } else {
                            panic!("PieceByte length exceeded chunk size");
                        }
                    }
                    Ok(None) => break, // EOF
                    Err(e) => panic!("Error reading file {}: {}", file, e),
                }
            }
        }
        if buffer_length > 0 {
            // calculate SHA1 hash of remaining piece_bytes
            let mut hasher = Sha1::new();
            hasher.update(&piece_bytes);
            let result = hasher.finalize();
            pieces.extend(&result);
            piece_count += 1u64;
        }

        println!("Total pieces: {}", piece_count);

        TrInfo {
            files: if !single_file { Some(tr_files) } else { None },
            length: if single_file {
                Some(metadata.len())
            } else {
                None
            },
            name: Some(name.to_string()),
            piece_length: chunk_size,
            pieces,
            private,
        }
    }

    fn read_chunk(reader: &mut BufReader<File>, chunk_size: usize) -> io::Result<Option<Vec<u8>>> {
        let mut buffer = vec![0u8; chunk_size];
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            return Ok(None); // EOF
        }
        buffer.truncate(n);
        Ok(Some(buffer))
    }

    fn bencode(&self) -> Vec<u8> {
        let mut bcode: Vec<u8> = Vec::new();
        bcode.push(b'd');
        if !self.files.is_none() {
            bcode.extend(bencode_string("files"));
            bcode.extend(bencode_file_list(&self.files.as_ref().unwrap()));
        }
        if !self.length.is_none() {
            bcode.extend(bencode_string("length"));
            bcode.extend(bencode_integer(self.length.unwrap()));
        }
        if !self.name.is_none() {
            bcode.extend(bencode_string("name"));
            bcode.extend(bencode_string(&self.name.as_ref().unwrap()));
        }
        bcode.extend(bencode_string("piece length"));
        bcode.extend(bencode_integer(self.piece_length));
        if !self.pieces.is_empty() {
            bcode.extend(bencode_string("pieces"));
            bcode.extend(bencode_bytes(&self.pieces));
        }
        if !self.private {
            bcode.extend(bencode_string("private"));
            bcode.extend(bencode_integer(1));
        }
        bcode.push(b'e');
        bcode
    }

    fn hash(&self) -> String {
        let bencoded_info = self.bencode();
        let mut hasher = Sha1::new();
        hasher.update(&bencoded_info);
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
        prcess_mode: ProcessMode,

        announce_list: Option<Vec<String>>,
        comment: Option<String>,
        creation_date: Option<u64>,
        created_by: Option<String>,
        private: bool,
        encoding: String,
    ) -> Self {
        let announce = if !announce_list.is_none() {
            Some(announce_list.as_ref().unwrap()[0].clone())
        } else {
            None
        };

        let piece_size = piece_size.or(Some(16)).unwrap();

        let announce_list = if !announce_list.is_none() {
            let mut alist: Vec<Vec<String>> = Vec::new();
            for url in announce_list.as_ref().unwrap() {
                alist.push(vec![url.clone()]);
            }
            Some(alist)
        } else {
            None
        };

        let info = match prcess_mode {
            ProcessMode::Create => {
                if target_path.is_none() {
                    panic!("Target path is required for creating a torrent");
                }
                TrInfo::new(target_path.clone().unwrap(), piece_size, private)
            }
            ProcessMode::Verify => None.unwrap(), // TODO: implement verify mode
        };

        let torrent = Torrent {
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
        };

        torrent
    }

    fn bencode(&self) -> Vec<u8> {
        let mut bcode: Vec<u8> = Vec::new();
        bcode.push(b'd');
        if !self.announce.is_none() {
            bcode.extend(bencode_string("announce"));
            bcode.extend(bencode_string(&self.announce.as_ref().unwrap()));
        }
        if !self.announce_list.is_none() {
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
        if !self.comment.is_none() {
            bcode.extend(bencode_string("comment"));
            bcode.extend(bencode_string(&self.comment.as_ref().unwrap()));
        }
        if !self.created_by.is_none() {
            bcode.extend(bencode_string("created by"));
            bcode.extend(bencode_string(&self.created_by.as_ref().unwrap()));
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

    pub fn write_to_file(&self) -> io::Result<()> {
        let bencoded_torrent = self.bencode();
        let mut file = File::create(&self.torrent_path)?;
        file.write_all(&bencoded_torrent)?;
        Ok(())
    }
}
