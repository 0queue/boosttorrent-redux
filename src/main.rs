use crate::bencode::de::deserialize;

mod bencode;

fn main() {
    println!("{:?}", deserialize(b"i32e").unwrap());
}
