use std::fs::{File, read};
use std::io::{Error as ioError, ErrorKind, Write};
use std::path::Path;

use crate::codec::{Value, bencode_int, bencode_string, decode};
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
        if !force && torrent_path.exists() {
            return Err(
                ioError::new(ErrorKind::AlreadyExists, "output file already exists").into(),
            );
        }
        let mut file = File::create(torrent_path)?;
        file.write_all(&self.to_bytes()?)?;
        Ok(())
    }

    pub fn read_torrent(tr_path: impl AsRef<Path>) -> TrResult<Self> {
        let bcode = read(tr_path)?;
        Self::from_bytes(&bcode)
    }

    pub fn from_bytes(bcode: &[u8]) -> TrResult<Self> {
        let root = decode(bcode)?;
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
                        Some(Value::Int(i)) => *i,
                        _ => {
                            return Err(TrError::InvalidTorrent(String::from(
                                "file length invalid",
                            )));
                        }
                    };
                    let path = match m.get("path") {
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

        let length = match info_dict.get("length") {
            Some(Value::Int(i)) => Some(*i),
            _ => None,
        };
        let name = match info_dict.get("name") {
            Some(Value::Bytes(b)) => Some(String::from_utf8(b.to_vec())?),
            _ => None,
        };
        let piece_length = match info_dict.get("piece length") {
            Some(Value::Int(i)) => *i,
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
        let private = match info_dict.get("private") {
            Some(Value::Int(i)) => *i != 0,
            _ => false,
        };
        let source = match info_dict.get("source") {
            Some(Value::Bytes(b)) => Some(String::from_utf8(b.to_vec())?),
            _ => None,
        };
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
            announce: match tr_dict.get("announce") {
                Some(Value::Bytes(b)) => Some(String::from_utf8(b.to_vec())?),
                _ => None,
            },
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
            comment: match tr_dict.get("comment") {
                Some(Value::Bytes(b)) => Some(String::from_utf8(b.to_vec())?),
                _ => None,
            },
            created_by: match tr_dict.get("created by") {
                Some(Value::Bytes(b)) => Some(String::from_utf8(b.to_vec())?),
                _ => None,
            },
            creation_date: match tr_dict.get("creation date") {
                Some(Value::UInt(i)) => Some(*i),
                Some(Value::Int(i)) => Some(*i as i64),
                _ => None,
            },
            encoding: match tr_dict.get("encoding") {
                Some(Value::Bytes(b)) => Some(String::from_utf8(b.to_vec())?),
                _ => None,
            },
            hash: match tr_dict.get("hash") {
                Some(Value::Bytes(b)) => Some(String::from_utf8(b.to_vec())?),
                _ => None,
            },
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
        let mut bcode: Vec<u8> = Vec::new();
        bcode.push(b'd');
        if let Some(announce) = &self.announce {
            bcode.extend(bencode_string("announce"));
            bcode.extend(bencode_string(announce));
        }
        if let Some(announce_list) = &self.announce_list {
            bcode.extend(bencode_string("announce-list"));
            bcode.push(b'l');
            for tier in announce_list {
                bcode.push(b'l');
                for url in tier {
                    bcode.extend(bencode_string(url));
                }
                bcode.push(b'e');
            }
            bcode.push(b'e');
        }
        if let Some(comment) = &self.comment {
            bcode.extend(bencode_string("comment"));
            bcode.extend(bencode_string(comment));
        }
        if let Some(created_by) = &self.created_by {
            bcode.extend(bencode_string("created by"));
            bcode.extend(bencode_string(created_by));
        }
        if let Some(creation_date) = self.creation_date {
            bcode.extend(bencode_string("creation date"));
            bcode.extend(bencode_int(creation_date));
        }
        if let Some(encoding) = &self.encoding {
            bcode.extend(bencode_string("encoding"));
            bcode.extend(bencode_string(encoding));
        }
        if let Some(hash) = &self.hash {
            bcode.extend(bencode_string("hash"));
            bcode.extend(bencode_string(hash));
        }
        if let Some(info) = &self.info {
            bcode.extend(bencode_string("info"));
            bcode.extend(info.bencode());
        } else {
            return Err(TrError::MissingField("info"));
        }
        bcode.push(b'e');
        Ok(bcode)
    }
}
