use std::path::PathBuf;

use structopt::StructOpt;

use crate::bencode::de::deserialize;

mod bencode;

#[derive(Debug, StructOpt)]
#[structopt()]
struct Args {
    #[structopt(parse(from_os_str))]
    torrent_file_path: PathBuf,
}

fn main() {
    let args: Args = Args::from_args();
    println!("{:?}", args);

    let contents = std::fs::read(args.torrent_file_path).unwrap();

    println!("{:?}", &deserialize(&contents).unwrap());
}
