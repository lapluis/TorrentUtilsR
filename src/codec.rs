use std::collections::HashMap;
use std::io::Write;

use crate::error::{BencodeError, TrResult};

const MAX_BENCODE_DEPTH: usize = 128;

pub(crate) enum Value<'a> {
    Uint(usize),
    NegInt(i64),
    Bytes(&'a [u8]),
    List(Vec<Value<'a>>),
    Dict(HashMap<String, Value<'a>>),
}

pub(crate) fn decode_with_info_slice(data: &[u8]) -> TrResult<(Value<'_>, Option<&[u8]>)> {
    let mut pos = 0;
    let mut info_slice = None;
    let value = parse(data, &mut pos, 0, &mut info_slice)?;
    if pos != data.len() {
        return Err(BencodeError::TrailingData.into());
    }
    Ok((value, info_slice))
}

fn is_valid_bencode_digits(s: &str, allow_zero: bool) -> bool {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if s.starts_with('0') {
        allow_zero && s.len() == 1
    } else {
        true
    }
}

fn parse<'a>(
    data: &'a [u8],
    pos: &mut usize,
    depth: usize,
    info_slice: &mut Option<&'a [u8]>,
) -> Result<Value<'a>, BencodeError> {
    if depth > MAX_BENCODE_DEPTH {
        return Err(BencodeError::MaxDepthExceeded);
    }

    match data.get(*pos) {
        Some(b'i') => {
            *pos += 1;
            let start = *pos;
            while *pos < data.len() && data[*pos] != b'e' {
                *pos += 1;
            }
            if *pos >= data.len() {
                return Err(BencodeError::UnterminatedInteger);
            }
            let num_str = std::str::from_utf8(&data[start..*pos])
                .map_err(|_| BencodeError::InvalidUtf8("integer"))?;
            *pos += 1;
            if let Some(digits) = num_str.strip_prefix('-') {
                if !is_valid_bencode_digits(digits, false) {
                    return Err(BencodeError::InvalidInteger);
                }
                let val = num_str
                    .parse::<i64>()
                    .map_err(|_| BencodeError::InvalidInteger)?;
                Ok(Value::NegInt(val))
            } else {
                if !is_valid_bencode_digits(num_str, true) {
                    return Err(BencodeError::InvalidInteger);
                }
                let val = num_str
                    .parse::<usize>()
                    .map_err(|_| BencodeError::InvalidInteger)?;
                Ok(Value::Uint(val))
            }
        }
        Some(b'l') => {
            *pos += 1;
            let mut items = Vec::new();
            while data.get(*pos) != Some(&b'e') {
                items.push(parse(data, pos, depth + 1, info_slice)?);
            }
            *pos += 1;
            Ok(Value::List(items))
        }
        Some(b'd') => {
            *pos += 1;
            let mut map = HashMap::new();
            while data.get(*pos) != Some(&b'e') {
                let key = match parse(data, pos, depth + 1, info_slice)? {
                    Value::Bytes(b) => String::from_utf8(b.to_vec())
                        .map_err(|_| BencodeError::InvalidUtf8("dictionary key"))?,
                    _ => return Err(BencodeError::DictKeyNotString),
                };
                let val_start = *pos;
                let val = parse(data, pos, depth + 1, info_slice)?;
                if depth == 0 && key == "info" {
                    *info_slice = Some(&data[val_start..*pos]);
                }
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
                return Err(BencodeError::TruncatedStringLength);
            }
            let len_str = std::str::from_utf8(&data[start..*pos])
                .map_err(|_| BencodeError::InvalidUtf8("string length"))?;
            if !is_valid_bencode_digits(len_str, true) {
                return Err(BencodeError::InvalidStringLength);
            }
            let len = len_str
                .parse::<usize>()
                .map_err(|_| BencodeError::InvalidStringLength)?;
            *pos += 1;
            let end = (*pos)
                .checked_add(len)
                .ok_or(BencodeError::StringLengthOverflow)?;
            if end > data.len() {
                return Err(BencodeError::TruncatedString);
            }
            let slice = &data[*pos..end];
            *pos = end;
            Ok(Value::Bytes(slice))
        }
        Some(byte) => Err(BencodeError::UnexpectedToken(*byte)),
        None => Err(BencodeError::UnexpectedEof),
    }
}

pub(crate) fn write_bencode_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    let _ = write!(out, "{}:", bytes.len());
    out.extend_from_slice(bytes);
}

pub(crate) fn write_bencode_string(out: &mut Vec<u8>, s: &str) {
    write_bencode_bytes(out, s.as_bytes());
}

pub(crate) fn write_bencode_uint(out: &mut Vec<u8>, value: usize) {
    let _ = write!(out, "i{value}e");
}

pub(crate) fn write_bencode_int(out: &mut Vec<u8>, value: i64) {
    let _ = write!(out, "i{value}e");
}

pub(crate) fn write_bencode_string_list(out: &mut Vec<u8>, list: &[String]) {
    out.push(b'l');
    for item in list {
        write_bencode_string(out, item);
    }
    out.push(b'e');
}
