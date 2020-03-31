use std::cmp;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::Formatter;

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

impl std::error::Error for BErr {}

impl std::fmt::Display for BErr {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl BVal {
    pub fn get(&self, key: &str) -> &BVal {
        if let BVal::Dict(d) = self {
            return d.get(&key.as_bytes().to_vec()).expect("Key not found");
        }

        panic!("Not a Dict")
    }

    pub fn string(&self) -> String {
        if let BVal::String(s) = self {
            return match std::str::from_utf8(s) {
                Ok(utf8) => utf8.to_string(),
                Err(_) => {
                    let mut res = String::new();
                    for byte in s {
                        res.push_str(&format!("{:02x}", byte));
                    }
                    res
                }
            };
        }

        panic!("Not a string");
    }

    pub fn bytes(&self) -> &[u8] {
        if let BVal::String(bytes) = self {
            return bytes;
        }

        panic!("Not a string");
    }
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
