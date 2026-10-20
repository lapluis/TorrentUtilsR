use std::collections::HashMap;

use crate::error::{BencodeError, TrResult};

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
        return Err(BencodeError::TrailingData.into());
    }
    Ok(value)
}

fn parse<'a>(data: &'a [u8], pos: &mut usize) -> Result<Value<'a>, BencodeError> {
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
            if num_str.starts_with("-") {
                let val = num_str
                    .parse::<i64>()
                    .map_err(|_| BencodeError::InvalidInteger)?;
                Ok(Value::UInt(val))
            } else {
                let val = num_str
                    .parse::<usize>()
                    .map_err(|_| BencodeError::InvalidInteger)?;
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
                        .map_err(|_| BencodeError::InvalidUtf8("dictionary key"))?,
                    _ => return Err(BencodeError::DictKeyNotString),
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
                return Err(BencodeError::TruncatedStringLength);
            }
            let len_str = std::str::from_utf8(&data[start..*pos])
                .map_err(|_| BencodeError::InvalidUtf8("string length"))?;
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

pub(crate) fn bencode_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut bcode: Vec<u8> = Vec::new();
    let len = bytes.len();
    bcode.extend(len.to_string().as_bytes());
    bcode.push(b':');
    bcode.extend(bytes);
    bcode
}

pub(crate) fn bencode_string(s: &str) -> Vec<u8> {
    bencode_bytes(s.as_bytes())
}

fn bencode_integer(value: impl ToString) -> Vec<u8> {
    let value = value.to_string();
    let mut bcode: Vec<u8> = Vec::new();
    bcode.push(b'i');
    bcode.extend(value.as_bytes());
    bcode.push(b'e');
    bcode
}

pub(crate) fn bencode_uint(i: usize) -> Vec<u8> {
    bencode_integer(i)
}

pub(crate) fn bencode_int(i: i64) -> Vec<u8> {
    bencode_integer(i)
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
