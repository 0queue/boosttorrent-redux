use std::collections::HashMap;
use crate::bencode::BVal;
use crate::bencode::ser::serialize;

#[test]
fn test_string() {
    let spam = BVal::String(b"spam".to_vec());
    assert_eq!(b"4:spam".to_vec(), serialize(&spam));
}

#[test]
fn test_integer() {
    let i = BVal::Integer(100);
    assert_eq!(b"i100e".to_vec(), serialize(&i));
}

#[test]
fn test_list() {
    let list = BVal::List(vec![
        BVal::String(b"hello".to_vec()),
        BVal::Integer(24)
    ]);
    assert_eq!(b"l5:helloi24ee".to_vec(), serialize(&list));
}

#[test]
fn test_dict() {
    let dict = BVal::Dict({
        let mut map = HashMap::new();
        map.insert(b"hello".to_vec(), BVal::String(b"world".to_vec()));
        map.insert(b"key2".to_vec(), BVal::List(vec![BVal::Integer(0), BVal::Integer(-5)]));
        map
    });
    let expected = b"d5:hello5:world4:key2li0ei-5eee".to_vec();
    assert_eq!(expected, serialize(&dict));
}