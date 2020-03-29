use crate::bencode::{BVal, compare_bytes_slice};

#[cfg(test)]
mod test;

pub fn serialize(bval: &BVal) -> Vec<u8> {
    let mut res: Vec<u8> = Vec::new();

    match bval {
        BVal::String(s) => {
            res.append(&mut s.len().to_string().as_bytes().to_vec());
            res.push(b':');
            res.append(&mut s.clone());
        }
        BVal::Integer(i) => {
            res.append(&mut format!("i{}e", i).as_bytes().to_vec())
        }
        BVal::List(l) => {
            res.push(b'l');
            l.into_iter().for_each(|e| {
                res.append(&mut serialize(e));
            });
            res.push(b'e');
        }
        BVal::Dict(d) => {
            res.push(b'd');
            let mut keys = d.keys().collect::<Vec<_>>();
            keys.sort_by(|a, b| compare_bytes_slice(a, b));
            for k in keys {
                res.append(&mut serialize(&BVal::String(k.clone())));
                res.append(&mut serialize(d.get(k).unwrap()))
            }
            res.push(b'e')
        }
    }

    res
}