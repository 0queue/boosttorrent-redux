use crate::bencode::compare_bytes_slice;

use super::*;

#[test]
fn test_parse_integer_literal() {
    let mut s123 = b"123e".to_vec();
    let res = parse_integer_literal(s123.as_mut()).unwrap();
    assert_eq!(res, 123);
    assert_eq!(s123, vec![b'e'])
}

#[test]
fn test_parse_string() {
    let mut s1 = b"4:spam".to_vec();

    let val = parse_string(s1.as_mut()).unwrap();

    assert_eq!(
        val,
        BVal::String(vec!['s' as u8, 'p' as u8, 'a' as u8, 'm' as u8])
    );
    assert_eq!(0, s1.len());
}

#[test]
fn test_parse_integer() {
    let mut s1 = b"i123e".to_vec();
    let mut s2 = b"i-4e".to_vec();
    let mut s3 = b"i0e".to_vec();

    let val1 = parse_integer(s1.as_mut()).unwrap();
    let val2 = parse_integer(s2.as_mut()).unwrap();
    let val3 = parse_integer(s3.as_mut()).unwrap();

    assert_eq!(val1, BVal::Integer(123));
    assert_eq!(val2, BVal::Integer(-4));
    assert_eq!(val3, BVal::Integer(0));
    assert_eq!(0, s1.len());
    assert_eq!(0, s2.len());
    assert_eq!(0, s3.len());
}

// TODO too lenient
// #[test]
// fn test_parse_integer_negative_zero() {
//     let mut s1 = b"i-0e".to_vec();
//     assert_eq!(parse_integer(s1.as_mut()), Err(BErr::InvalidInteger));
//
// }

// TODO too lenient
// #[test]
// fn test_parse_integer_leading_zero() {
//     let mut s1 = b"i023e".to_vec();
//     assert_eq!(parse_integer(s1.as_mut()), Err(BErr::InvalidInteger));
// }

#[test]
fn test_parse_list() {
    let mut s1 = b"l4:spami123ee".to_vec();

    let val1 = parse_list(s1.as_mut()).unwrap();

    assert_eq!(
        val1,
        BVal::List(vec![
            BVal::String(vec!['s' as u8, 'p' as u8, 'a' as u8, 'm' as u8]),
            BVal::Integer(123)
        ])
    )
}

#[test]
fn test_parses_dict() {
    let mut s1 = b"d5:hello5:world4:spami123ee".to_vec();
    let val1 = parse_dict(s1.as_mut()).unwrap();

    let mut map = HashMap::new();
    map.insert(
        vec!['h' as u8, 'e' as u8, 'l' as u8, 'l' as u8, 'o' as u8],
        BVal::String(vec!['w' as u8, 'o' as u8, 'r' as u8, 'l' as u8, 'd' as u8]),
    );
    map.insert(
        vec!['s' as u8, 'p' as u8, 'a' as u8, 'm' as u8],
        BVal::Integer(123),
    );
    assert_eq!(val1, BVal::Dict(map));
}

#[test]
fn test_parse_dict_not_ascending() {
    let mut s1 = b"d5:worldi1e5:helloi2ee".to_vec();
    assert_eq!(parse_dict(s1.as_mut()), Err(BErr::InvalidDict));
}

#[test]
fn test_compare_string() {
    let v1 = vec![0, 1, 2, 3];
    let v2 = vec![1, 1, 2, 3];
    let v3 = vec![9, 8, 7];
    let v4 = vec![9, 8, 8];
    let vs = vec![8];
    let vl = vec![8, 8, 8];

    assert_eq!(
        Ordering::Equal,
        compare_bytes_slice(v1.as_ref(), v1.as_ref())
    );
    assert_eq!(
        Ordering::Less,
        compare_bytes_slice(v1.as_ref(), v2.as_ref())
    );
    assert_eq!(
        Ordering::Greater,
        compare_bytes_slice(v2.as_ref(), v1.as_ref())
    );
    assert_eq!(
        Ordering::Less,
        compare_bytes_slice(v3.as_ref(), v4.as_ref())
    );
    assert_eq!(
        Ordering::Greater,
        compare_bytes_slice(v4.as_ref(), v3.as_ref())
    );
    assert_eq!(
        Ordering::Less,
        compare_bytes_slice(vs.as_ref(), vl.as_ref())
    );
    assert_eq!(
        Ordering::Greater,
        compare_bytes_slice(vl.as_ref(), vs.as_ref())
    );
}
