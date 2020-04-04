use std::path::PathBuf;
use std::str::FromStr;

use async_std::net::{Ipv4Addr, SocketAddrV4};
use async_std::sync::Arc;
use crossbeam::queue::SegQueue;
use flume::Receiver;
use rand::Rng;
use structopt::StructOpt;

use crate::bencode::de::deserialize;
use crate::counter::Counter;

mod bencode;
mod counter;
mod peer;

#[derive(Debug, StructOpt)]
#[structopt()]
struct Args {
    #[structopt(parse(from_os_str))]
    torrent_file_path: PathBuf,
}

const NUM_PEERS: usize = 5;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Args = Args::from_args();
    println!("{:?}", args);

    let contents = std::fs::read(args.torrent_file_path)?;

    let torrent = deserialize(&contents)?;

    let id = gen_peer_id();
    println!("This is {:02x?}", id);
    println!("announce: {:?}", torrent.get("announce").string());

    let (done_tx, done_rx) = flume::unbounded::<PieceMeta>();

    let work_queue: Arc<SegQueue<PieceMeta>> = Arc::new(SegQueue::new());
    let (counter, counter_tx) = Counter::new();

    println!("file name: {:?}", torrent.get("info").get("name").string());
    println!("file length: {:?}", torrent.get("info").get("length"));
    println!(
        "piece length: {:?}",
        torrent.get("info").get("piece length")
    );

    let piece_hashes = torrent.get("info").get("pieces").bytes();

    println!(
        "Length of pieces array: {} multiple of 20? {}",
        piece_hashes.len(),
        piece_hashes.len() % 20 == 0
    );

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
            PieceMeta { index, hash }
        })
        .collect::<Vec<_>>();

    let pieces = {
        use rand::seq::SliceRandom;

        let mut rng = rand::thread_rng();
        pieces.shuffle(&mut rng);
        pieces
    };

    for p in pieces {
        work_queue.push(p);
    }

    println!("Starting processing");
    async_std::task::block_on(async move {
        // TODO:
        //  - Pipelining in peers?

        let addresses = vec![
            SocketAddrV4::new(Ipv4Addr::from_str("127.0.0.1").unwrap(), 8080),
            SocketAddrV4::new(Ipv4Addr::from_str("127.0.0.1").unwrap(), 8081),
        ];

        let peers_handle = async_std::task::spawn(peer::async_std_spawner(
            addresses,
            work_queue.clone(),
            done_tx,
            counter_tx,
            id,
        ));

        let writer_handle = async_std::task::spawn(data_writer(done_rx));
        let counter_handle = async_std::task::spawn(counter.start());

        writer_handle.await;
        peers_handle.await;

        println!(
            "Counter: {:#?}\nwork_queue.len(): {}",
            counter_handle.await,
            work_queue.len()
        );
    });

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

#[derive(Debug)]
pub struct PieceMeta {
    pub index: usize,
    pub hash: [u8; 20],
}

async fn data_writer(mut done_rx: Receiver<PieceMeta>) {
    println!("Starting finished work receiver");
    loop {
        match done_rx.recv_async().await {
            Ok(p) => {
                println!("Finished with {:?}", p.index);
            }
            Err(_) => {
                println!("all senders disconnected");
                break;
            }
        }
    }
    println!("Done");
}
