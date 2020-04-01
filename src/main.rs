use std::path::PathBuf;

use async_std::sync::Arc;
use crossbeam::queue::SegQueue;
use flume::Receiver;
use rand::Rng;
use structopt::StructOpt;

use crate::bencode::de::deserialize;
use crate::counter::Counter;
use crate::peer::Peer;

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

    let (done_tx, done_rx) = flume::unbounded::<Piece>();

    let work_queue: Arc<SegQueue<Piece>> = Arc::new(SegQueue::new());
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
            Piece { index, hash }
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
        let mut children = vec![];
        for i in 0..NUM_PEERS {
            let peer = Peer {
                address: i,
                work_queue: work_queue.clone(),
                done_channel: done_tx.clone(),
                counter_channel: counter_tx.clone(),
            };

            children.push(async_std::task::spawn(peer.start()));
        }

        // seems like senders should be dropped after being distributed
        drop(done_tx);
        drop(counter_tx);

        let writer_handle = async_std::task::spawn(data_writer(done_rx));
        let counter_handle = async_std::task::spawn(counter.start());

        futures::future::join_all(children).await;
        writer_handle.await;

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
pub struct Piece {
    pub index: usize,
    pub hash: [u8; 20],
}

async fn data_writer(mut done_rx: Receiver<Piece>) {
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