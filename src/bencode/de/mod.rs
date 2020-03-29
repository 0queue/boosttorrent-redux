use std::cmp;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::str::FromStr;

use crate::bencode::{BErr, BVal, compare_bytes_slice};

#[cfg(test)]
mod test;

pub fn deserialize(bytes: &[u8]) -> Result<BVal, BErr> {
    parse_val(&mut bytes.clone().to_vec())
}

fn parse_val(stack: &mut Vec<u8>) -> Result<BVal, BErr> {
    match stack.get(0) {
        Some(b) if (b'0'..=b'9').contains(b) => parse_string(stack),
        Some(b'i') => parse_integer(stack),
        Some(b'l') => parse_list(stack),
        Some(b'd') => parse_dict(stack),
        _ => Err(BErr::InvalidBVal)
    }
}

fn parse_string(stack: &mut Vec<u8>) -> Result<BVal, BErr> {
    let len = parse_integer_literal(stack)? as usize;

    if pop_front(stack) != Some(b':') {
        return Err(BErr::InvalidString);
    }

    let bstring = {
        let mut bstring = Vec::with_capacity(len);
        for _ in 0..len {
            match pop_front(stack) {
                Some(b) => bstring.push(b),
                None => return Err(BErr::InvalidString)
            }
        }
        bstring
    };

    Ok(BVal::String(bstring))
}

fn parse_integer(stack: &mut Vec<u8>) -> Result<BVal, BErr> {
    if pop_front(stack) != Some(b'i') {
        return Err(BErr::InvalidInteger);
    }

    let is_negative = stack.get(0) == Some(&b'-');
    if is_negative {
        stack.remove(0);
    }

    // allows a little more than the spec (such as leading zeroes) but whatever
    let uint = parse_integer_literal(stack)?;

    if Some(b'e') != pop_front(stack) {
        return Err(BErr::InvalidInteger);
    }

    let signed = uint as i64 * if is_negative { -1 } else { 1 };
    Ok(BVal::Integer(signed))
}

fn parse_list(stack: &mut Vec<u8>) -> Result<BVal, BErr> {
    if pop_front(stack) != Some(b'l') {
        return Err(BErr::InvalidList);
    }

    let mut res: Vec<BVal> = Vec::new();
    while stack.len() > 0 && stack[0] != b'e' {
        res.push(parse_val(stack)?);
    }

    if pop_front(stack) != Some(b'e') {
        return Err(BErr::InvalidList);
    }

    Ok(BVal::List(res))
}

fn parse_dict(stack: &mut Vec<u8>) -> Result<BVal, BErr> {
    if pop_front(stack) != Some(b'd') {
        return Err(BErr::InvalidDict);
    }

    let mut last_key: Option<Vec<u8>> = None;
    let mut res: HashMap<Vec<u8>, BVal> = HashMap::new();
    while stack.len() > 0 && stack[0] != b'e' {
        let key = match parse_string(stack)? {
            BVal::String(s) => s,
            _ => return Err(BErr::InvalidDict)
        };
        let val = parse_val(stack)?;

        match last_key {
            None => last_key = Some(key.clone()),
            Some(last) => match compare_bytes_slice(&last, &key) {
                Ordering::Less => last_key = Some(key.clone()),
                _ => return Err(BErr::InvalidDict)
            }
        }

        res.insert(key, val);
    }

    if pop_front(stack) != Some(b'e') {
        return Err(BErr::InvalidDict);
    }

    Ok(BVal::Dict(res))
}

fn parse_integer_literal(stack: &mut Vec<u8>) -> Result<u64, BErr> {
    let mut buf = String::new();
    while stack.len() > 0 && (b'0'..=b'9').contains(&stack[0]) {
        buf.push(stack.remove(0) as char) // valid utf-8 because of range check
    }

    let len = u64::from_str(buf.as_str())
        .map_err(|_| BErr::InvalidIntegerLiteral)?;

    Ok(len)
}

fn pop_front(stack: &mut Vec<u8>) -> Option<u8> {
    match stack.get(0) {
        None => None,
        Some(..) => Some(stack.remove(0)),
    }
}