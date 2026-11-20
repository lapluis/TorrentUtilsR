use std::collections::HashMap;
use std::fs::{File, OpenOptions, read};
use std::io::{Error as ioError, ErrorKind, Write};
use std::path::Path;

use sha1::{Digest, Sha1};

use crate::codec::{Value, decode_with_info_slice, write_bencode_int, write_bencode_string};
use crate::error::{TrError, TrResult};
use crate::progress::ProgressReporter;
use crate::tr_file::TrFile;
use crate::tr_info::{CreateOptions, TrInfo};

#[derive(Clone, Debug)]
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
        target_path: impl AsRef<Path>,
        tr_config: &CreateOptions,
        progress: Option<&dyn ProgressReporter>,
    ) -> TrResult<()> {
        let info = TrInfo::new(target_path, tr_config, progress)?;
        self.hash = Some(info.hash());
        self.info = Some(info);
        Ok(())
    }

    pub fn write_to_file(&self, torrent_path: impl AsRef<Path>, force: bool) -> TrResult<()> {
        let torrent_path = torrent_path.as_ref();
        let bytes = self.to_bytes()?;
        let mut file = if force {
            File::create(torrent_path)?
        } else {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(torrent_path)
                .map_err(|err| {
                    if err.kind() == ErrorKind::AlreadyExists {
                        ioError::new(ErrorKind::AlreadyExists, "output file already exists")
                    } else {
                        err
                    }
                })?
        };
        file.write_all(&bytes)?;
        Ok(())
    }

    pub fn read_torrent(tr_path: impl AsRef<Path>) -> TrResult<Self> {
        let bcode = read(tr_path)?;
        Self::from_bytes(&bcode)
    }

    pub fn from_bytes(bcode: &[u8]) -> TrResult<Self> {
        let (root, raw_info_bytes) = decode_with_info_slice(bcode)?;
        let tr_dict = match root {
            Value::Dict(m) => m,
            _ => {
                return Err(TrError::InvalidTorrent(String::from(
                    "torrent root is not a dictionary",
                )));
            }
        };

        let info_dict = match tr_dict.get("info") {
            Some(Value::Dict(m)) => m,
            _ => {
                return Err(TrError::InvalidTorrent(String::from("missing info dict")));
            }
        };

        let tr_files = match info_dict.get("files") {
            Some(Value::List(files)) => {
                let mut out = Vec::new();
                for file in files {
                    let Value::Dict(m) = file else {
                        return Err(TrError::InvalidTorrent(String::from(
                            "files entry is not a dictionary",
                        )));
                    };
                    let length = match m.get("length") {
                        Some(Value::Uint(i)) => *i,
                        _ => {
                            return Err(TrError::InvalidTorrent(String::from(
                                "file length invalid",
                            )));
                        }
                    };
                    // Older clients may store local-encoding bytes in `path`
                    // alongside an explicitly UTF-8 encoded alternative.
                    let path = match m.get("path.utf-8").or_else(|| m.get("path")) {
                        Some(Value::List(parts)) => {
                            let mut ps = Vec::new();
                            for part in parts {
                                let Value::Bytes(b) = part else {
                                    return Err(TrError::InvalidTorrent(String::from(
                                        "file path component is not a string",
                                    )));
                                };
                                ps.push(String::from_utf8(b.to_vec())?);
                            }
                            ps
                        }
                        _ => {
                            return Err(TrError::InvalidTorrent(String::from("file path invalid")));
                        }
                    };
                    out.push(TrFile::new(length, path));
                }
                Some(out)
            }
            _ => None,
        };

        let length = optional_int(info_dict, "length");
        let name = optional_string(
            info_dict,
            if info_dict.contains_key("name.utf-8") {
                "name.utf-8"
            } else {
                "name"
            },
        )?;
        let piece_length = match info_dict.get("piece length") {
            Some(Value::Uint(i)) => *i,
            _ => {
                return Err(TrError::InvalidTorrent(String::from(
                    "piece length missing",
                )));
            }
        };
        let pieces = match info_dict.get("pieces") {
            Some(Value::Bytes(b)) => b.to_vec(),
            _ => return Err(TrError::InvalidTorrent(String::from("pieces missing"))),
        };
        let private = optional_int(info_dict, "private").is_some_and(|value| value != 0);
        let source = optional_string(info_dict, "source")?;
        let tr_info = TrInfo::from_parts(
            tr_files,
            length,
            name,
            piece_length,
            pieces,
            private,
            source,
        )?;

        Ok(Torrent {
            announce: optional_string(&tr_dict, "announce")?,
            announce_list: match tr_dict.get("announce-list") {
                Some(Value::List(lists)) => {
                    let mut alist: Vec<Vec<String>> = Vec::new();
                    for tier in lists {
                        match tier {
                            Value::List(urls) => {
                                let mut tier_list: Vec<String> = Vec::new();
                                for url in urls {
                                    match url {
                                        Value::Bytes(b) => {
                                            tier_list.push(String::from_utf8(b.to_vec())?)
                                        }
                                        _ => {
                                            return Err(TrError::InvalidTorrent(String::from(
                                                "Announce URL is not a string",
                                            )));
                                        }
                                    }
                                }
                                alist.push(tier_list);
                            }
                            _ => {
                                return Err(TrError::InvalidTorrent(String::from(
                                    "Announce tier is not a list",
                                )));
                            }
                        }
                    }
                    Some(alist)
                }
                _ => None,
            },
            comment: optional_string(&tr_dict, "comment")?,
            created_by: optional_string(&tr_dict, "created by")?,
            creation_date: optional_i64(&tr_dict, "creation date"),
            encoding: optional_string(&tr_dict, "encoding")?,
            hash: optional_string(&tr_dict, "hash")?
                .or_else(|| raw_info_bytes.map(|bytes| hex::encode(Sha1::digest(bytes)))),
            info: Some(tr_info),
        })
    }

    pub fn get_info(&self) -> Option<&TrInfo> {
        self.info.as_ref()
    }

    pub fn announce(&self) -> Option<&str> {
        self.announce.as_deref()
    }

    pub fn announce_list(&self) -> Option<&[Vec<String>]> {
        self.announce_list.as_deref()
    }

    pub fn comment(&self) -> Option<&str> {
        self.comment.as_deref()
    }

    pub fn created_by(&self) -> Option<&str> {
        self.created_by.as_deref()
    }

    pub fn creation_date(&self) -> Option<i64> {
        self.creation_date
    }

    pub fn encoding(&self) -> Option<&str> {
        self.encoding.as_deref()
    }

    pub fn hash(&self) -> Option<&str> {
        self.hash.as_deref()
    }

    pub fn to_bytes(&self) -> TrResult<Vec<u8>> {
        let Some(info) = &self.info else {
            return Err(TrError::MissingField("info"));
        };
        let mut bcode = Vec::with_capacity(info.pieces().len().saturating_add(256));
        bcode.push(b'd');
        if let Some(announce) = &self.announce {
            write_bencode_string(&mut bcode, "announce");
            write_bencode_string(&mut bcode, announce);
        }
        if let Some(announce_list) = &self.announce_list {
            write_bencode_string(&mut bcode, "announce-list");
            bcode.push(b'l');
            for tier in announce_list {
                bcode.push(b'l');
                for url in tier {
                    write_bencode_string(&mut bcode, url);
                }
                bcode.push(b'e');
            }
            bcode.push(b'e');
        }
        if let Some(comment) = &self.comment {
            write_bencode_string(&mut bcode, "comment");
            write_bencode_string(&mut bcode, comment);
        }
        if let Some(created_by) = &self.created_by {
            write_bencode_string(&mut bcode, "created by");
            write_bencode_string(&mut bcode, created_by);
        }
        if let Some(creation_date) = self.creation_date {
            write_bencode_string(&mut bcode, "creation date");
            write_bencode_int(&mut bcode, creation_date);
        }
        if let Some(encoding) = &self.encoding {
            write_bencode_string(&mut bcode, "encoding");
            write_bencode_string(&mut bcode, encoding);
        }
        if let Some(hash) = &self.hash {
            write_bencode_string(&mut bcode, "hash");
            write_bencode_string(&mut bcode, hash);
        }
        write_bencode_string(&mut bcode, "info");
        info.write_bencode(&mut bcode);
        bcode.push(b'e');
        Ok(bcode)
    }
}

fn optional_string(dict: &HashMap<String, Value<'_>>, key: &str) -> TrResult<Option<String>> {
    match dict.get(key) {
        Some(Value::Bytes(bytes)) => Ok(Some(String::from_utf8(bytes.to_vec())?)),
        _ => Ok(None),
    }
}

fn optional_int(dict: &HashMap<String, Value<'_>>, key: &str) -> Option<usize> {
    match dict.get(key) {
        Some(Value::Uint(value)) => Some(*value),
        _ => None,
    }
}

fn optional_i64(dict: &HashMap<String, Value<'_>>, key: &str) -> Option<i64> {
    match dict.get(key) {
        Some(Value::NegInt(value)) => Some(*value),
        Some(Value::Uint(value)) => Some(*value as i64),
        _ => None,
    }
}
