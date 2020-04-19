use std::cmp::min;
use std::io::stdout;
use std::io::Write;
use std::path::PathBuf;

use async_std::fs::File;
use async_std::net::Ipv4Addr;
use async_std::net::SocketAddrV4;
use async_std::path::Path;
use async_std::sync::Arc;
use async_std::sync::RwLock;
use byteorder::BigEndian;
use byteorder::ByteOrder;
use futures::AsyncWriteExt;
use md5::Digest;
use md5::Md5;
use rand::Rng;
use structopt::StructOpt;

use crate::bencode::BVal;
use crate::bencode::de::deserialize;
use crate::counter::Counter;
use crate::data::DownloadedPiece;
use crate::data::Lifecycle;
use crate::data::PeerBus;
use crate::data::State;
use crate::timer::Timer;

mod bencode;
mod broadcast;
mod counter;
mod data;
mod peer;
mod tracker;
mod timer;
mod bitvec_ext;

#[derive(Debug, StructOpt)]
#[structopt()]
struct Args {
    #[structopt(parse(from_os_str))]
    torrent_file_path: PathBuf,

    #[structopt(long)]
    md5sum: Option<String>,
}

// TODO small features to add:
//  * built in timing
//  * hash checking
//  - graceful ctrl c exits
//  - colored output (better logs in general)
//  * bitvec ext trait to add ones() and zeroes()
//  * unlimited peers
//  - correct reports to the tracker at the end
//  * add keep alives so that when the last peer dies with the last piece we can actually finish downloading
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut timer = Timer::new();
    timer.start();

    let args: Args = Args::from_args();
    let contents = std::fs::read(args.torrent_file_path.clone())?;
    let torrent = deserialize(&contents)?;

    let id = gen_peer_id();
    println!("This is {:02x?}", id);

    let file_hash = torrent["info"].hash();

    let addresses = {
        let response = tracker::announce(&torrent, &id, 6881, tracker::Event::Started);

        match &response["peers"] {
            BVal::String(peers) => peers
                .chunks(6)
                .map(|peer| {
                    let address = BigEndian::read_u32(peer);
                    let port = BigEndian::read_u16(&peer[4..]);
                    SocketAddrV4::new(Ipv4Addr::from(address), port)
                })
                .collect::<Vec<_>>(),
            BVal::Dict(_) => todo!("regular peer list"),
            _ => panic!("peers value not String or Dict"),
        }
    };

    println!("{} Addresses: {:?}", addresses.len(), addresses);

    let (done_tx, done_rx) = flume::unbounded::<DownloadedPiece>();

    let (counter, counter_tx) = Counter::new();
    let (endgame_tx, endgame_rx) = broadcast::unbounded();

    let mut output = async_std::task::block_on(async {
        let name = torrent["info"]["name"].string();
        if Path::new(&name).exists().await {
            println!("File already exists, overwriting: {}", name);
        }

        File::create(name).await.unwrap()
    });

    let md5sum = args.md5sum.as_ref().map(|s| hex::decode(s).unwrap());

    let piece_length = torrent["info"]["piece length"].integer();
    let piece_hashes = torrent["info"]["pieces"].bytes();
    let file_length = torrent["info"]["length"].integer() as usize;

    if piece_hashes.len() % 20 != 0 {
        panic!("piece hash array length not a multiple of 20");
    }

    let mut pieces = torrent
        .get("info")
        .get("pieces")
        .bytes()
        .chunks(20)
        .enumerate()
        .map(|(index, chunk)| {
            let mut hash = [0u8; 20];
            hash.copy_from_slice(chunk);
            let piece_length = {
                let start = index * piece_length as usize;
                let end = min(start + piece_length as usize, file_length);
                end - start
            };
            PieceMeta {
                index,
                hash,
                length: piece_length,
            }
        })
        .collect::<Vec<_>>();

    let pieces = {
        use rand::seq::SliceRandom;

        let mut rng = rand::thread_rng();
        pieces.shuffle(&mut rng);
        pieces
    };

    let num_pieces = pieces.len();

    let (work_tx, work_rx) = async_std::sync::channel(pieces.len());

    println!("Starting processing");
    async_std::task::block_on(async move {
        for p in pieces.to_vec() {
            work_tx.send(p).await;
        }

        let peer_bus = PeerBus {
            work_tx: work_tx.clone(),
            work_rx: work_rx.clone(),
            done_tx,
            counter_tx,
            endgame_rx,
        };
        let shared_state = Arc::new(RwLock::new(State {
            received: 0,
            total: num_pieces,
            lifecycle: Lifecycle::Downloading,
            id,
            file_hash,
        }));

        let peers_handle =
            async_std::task::spawn(peer::spawner(addresses, peer_bus, shared_state.clone()));
        let writer_handle = async_std::task::spawn(data::writer(
            piece_length,
            num_pieces,
            file_length,
            done_rx,
            endgame_tx,
            work_rx.clone(),
            shared_state.clone(),
        ));
        let counter_handle = async_std::task::spawn(counter.start());

        peers_handle.await;
        let data = {
            let data = writer_handle.await;

            if let Some(hash) = md5sum {
                let mut hasher = Md5::new();
                println!("Starting md5 hash");
                stdout().flush().unwrap();
                hasher.input(&data);
                let result = hasher.result();
                if result.as_slice() == hash.as_slice() {
                    println!("md5sum matches!");
                    Some(data)
                } else {
                    eprintln!("Expected {}", args.md5sum.unwrap());
                    eprintln!("Found    {}", hex::encode(&result));
                    None
                }
            } else {
                Some(data)
            }
        };

        if let Some(data) = data {
            println!("writing data");
            stdout().flush().unwrap();
            output.write_all(&data).await.unwrap();
            output.sync_all().await.unwrap();
        }

        println!(
            "Counter: {:#?}\nwork_queue.len(): {}",
            counter_handle.await,
            work_rx.len()
        );
    });

    tracker::announce(&torrent, &id, 6881, tracker::Event::Stopped);

    let total_time_secs = timer.time().unwrap().as_secs();
    let mins = total_time_secs / 60;
    let secs = total_time_secs % 60;
    println!("Total time: {}:{}", mins, secs);

    Ok(())
}

fn gen_peer_id() -> [u8; 20] {
    // Generate peer id in Azures style ("-<2 letter client code><4 digit version number>-<12 random digits>")
    let mut id = b"-BO0001-".to_vec();
    let mut rng = rand::prelude::thread_rng();
    for _ in 0..12 {
        id.push(rng.gen())
    }

    let mut res = [0u8; 20];
    res.copy_from_slice(&id);
    res
}

#[derive(Debug, Clone)]
pub struct PieceMeta {
    pub index: usize,
    pub hash: [u8; 20],
    pub length: usize,
}