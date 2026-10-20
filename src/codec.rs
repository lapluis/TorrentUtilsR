use std::collections::HashMap;

use crate::utils::{TrError, TrResult};

pub(crate) enum Value<'a> {
    Int(usize),
    UInt(i64),
    Bytes(&'a [u8]),
    List(Vec<Value<'a>>),
    Dict(HashMap<String, Value<'a>>),
}

pub(crate) fn decode(data: &[u8]) -> TrResult<Value<'_>> {
    let mut pos = 0;
    let value = parse(data, &mut pos)?;
    if pos != data.len() {
        return Err(TrError::InvalidTorrent(String::from(
            "trailing data after torrent",
        )));
    }
    Ok(value)
}

fn parse<'a>(data: &'a [u8], pos: &mut usize) -> TrResult<Value<'a>> {
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
            let num_str =
                std::str::from_utf8(&data[start..*pos]).map_err(|_| "invalid utf8 in int")?;
            *pos += 1;
            if num_str.starts_with("-") {
                let val = num_str.parse::<i64>().map_err(|_| "invalid int")?;
                Ok(Value::UInt(val))
            } else {
                let val = num_str.parse::<usize>().map_err(|_| "invalid int")?;
                Ok(Value::Int(val))
            }
        }
        Some(b'l') => {
            *pos += 1;
            let mut items = Vec::new();
            while data.get(*pos) != Some(&b'e') {
                items.push(parse(data, pos)?);
            }
            *pos += 1;
            Ok(Value::List(items))
        }
        Some(b'd') => {
            *pos += 1;
            let mut map = HashMap::new();
            while data.get(*pos) != Some(&b'e') {
                let key = match parse(data, pos)? {
                    Value::Bytes(b) => String::from_utf8(b.to_vec())
                        .map_err(|_| TrError::InvalidTorrent(String::from("invalid utf8 key")))?,
                    _ => {
                        return Err(TrError::InvalidTorrent(String::from("dict key not string")));
                    }
                };
                let val = parse(data, pos)?;
                map.insert(key, val);
            }
            *pos += 1;
            Ok(Value::Dict(map))
        }
        Some(b'0'..=b'9') => {
            let start = *pos;
            while *pos < data.len() && data[*pos] != b':' {
                *pos += 1;
            }
            if *pos >= data.len() {
                return Err(TrError::InvalidTorrent(String::from(
                    "truncated string length",
                )));
            }
            let len_str =
                std::str::from_utf8(&data[start..*pos]).map_err(|_| "invalid utf8 length")?;
            let len = len_str.parse::<usize>().map_err(|_| "bad string length")?;
            *pos += 1;
            let end = (*pos)
                .checked_add(len)
                .ok_or_else(|| TrError::InvalidTorrent(String::from("string length overflow")))?;
            if end > data.len() {
                return Err(TrError::InvalidTorrent(String::from("truncated string")));
            }
            let slice = &data[*pos..end];
            *pos = end;
            Ok(Value::Bytes(slice))
        }
        Some(_) => Err("unknown token".into()),
        None => Err("unexpected EOF".into()),
    }
}

pub(crate) fn bencode_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut bcode: Vec<u8> = Vec::new();
    let len = bytes.len();
    bcode.extend(len.to_string().as_bytes());
    bcode.push(b':');
    bcode.extend(bytes);
    bcode
}

pub(crate) fn bencode_string(s: &str) -> Vec<u8> {
    let mut bcode: Vec<u8> = Vec::new();
    let len = s.len();
    bcode.extend(len.to_string().as_bytes());
    bcode.push(b':');
    bcode.extend(s.as_bytes());
    bcode
}

pub(crate) fn bencode_uint(i: usize) -> Vec<u8> {
    let mut bcode: Vec<u8> = Vec::new();
    bcode.push(b'i');
    bcode.extend(i.to_string().as_bytes());
    bcode.push(b'e');
    bcode
}

pub(crate) fn bencode_int(i: i64) -> Vec<u8> {
    let mut bcode: Vec<u8> = Vec::new();
    bcode.push(b'i');
    bcode.extend(i.to_string().as_bytes());
    bcode.push(b'e');
    bcode
}

pub(crate) fn bencode_string_list(list: &[String]) -> Vec<u8> {
    let mut bcode: Vec<u8> = Vec::new();
    bcode.push(b'l');
    for item in list {
        bcode.extend(bencode_string(item));
    }
    bcode.push(b'e');
    bcode
}
