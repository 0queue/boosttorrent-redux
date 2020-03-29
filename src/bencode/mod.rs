use std::collections::HashMap;
use std::iter::Peekable;
use std::slice::IterMut;
use std::str::FromStr;
use std::cmp::Ordering;
use std::cmp;

pub mod de;
pub mod ser;

#[derive(Debug, PartialEq, Clone)]
pub enum BVal {
    String(Vec<u8>),
    Integer(i64),
    List(Vec<BVal>),
    Dict(HashMap<Vec<u8>, BVal>),
}

#[derive(Debug, PartialEq, Clone)]
pub enum BErr {
    InvalidBVal,
    InvalidString,
    InvalidInteger,
    InvalidIntegerLiteral,
    InvalidList,
    InvalidDict,
}

fn compare_bytes_slice(a: &[u8], b: &[u8]) -> Ordering {
    let len = cmp::min(a.len(), b.len());

    for i in 0..len {
        let res = a[i].cmp(&b[i]);
        if res != Ordering::Equal {
            return res;
        }
    }

    a.len().cmp(&b.len())
}