use std::path::PathBuf;

use async_std::fs::File;
use async_std::io::SeekFrom;
use async_std::net::Ipv4Addr;
use async_std::net::SocketAddrV4;
use async_std::path::Path;
use async_std::sync::Arc;
use byteorder::BigEndian;
use byteorder::ByteOrder;
use crossbeam::queue::SegQueue;
use flume::Receiver;
use futures::AsyncSeekExt;
use futures::AsyncWriteExt;
use rand::Rng;
use sha1::Digest;
use sha1::Sha1;
use structopt::StructOpt;

use crate::bencode::BVal;
use crate::bencode::de::deserialize;
use crate::counter::Counter;
use crate::peer::DownloadedPiece;
use crate::peer::PeerBus;
use crate::peer::Us;

mod bencode;
mod counter;
mod peer;
mod tracker;

#[derive(Debug, StructOpt)]
#[structopt()]
struct Args {
    #[structopt(parse(from_os_str))]
    torrent_file_path: PathBuf,
}

const NUM_PEERS: usize = 20;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Args = Args::from_args();
    let contents = std::fs::read(args.torrent_file_path)?;
    let torrent = deserialize(&contents)?;

    let id = gen_peer_id();
    println!("This is {:02x?}", id);

    let file_hash = {
        let bencoded = torrent["info"].serialize();
        let mut hasher = Sha1::new();
        hasher.input(&bencoded);
        let mut hash = [0u8; 20];
        hash.copy_from_slice(&hasher.result());
        hash
    };

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
    let (have_tx, have_rx) = crossbeam::unbounded();

    let work_queue: Arc<SegQueue<PieceMeta>> = Arc::new(SegQueue::new());
    let (counter, counter_tx) = Counter::new();

    let output = async_std::task::block_on(async {
        let name = torrent["info"]["name"].string();
        if Path::new(&name).exists().await {
            println!("File already exists, overwriting: {}", name);
        }

        File::create(name).await.unwrap()
    });

    let piece_length = torrent["info"]["piece length"].integer();
    let piece_hashes = torrent["info"]["pieces"].bytes();

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
            PieceMeta {
                index,
                hash,
                length: piece_length as usize,
            }
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
        let peer_bus = PeerBus {
            work_queue: work_queue.clone(),
            done_tx,
            counter_tx,
            have_tx,
            have_rx,
        };
        let us = Us { id, file_hash };

        let peers_handle = async_std::task::spawn(peer::async_std_spawner(addresses, peer_bus, us));
        let writer_handle = async_std::task::spawn(data_writer(output, piece_length, done_rx, work_queue.len()));
        let counter_handle = async_std::task::spawn(counter.start());

        writer_handle.await;
        peers_handle.await;

        println!(
            "Counter: {:#?}\nwork_queue.len(): {}",
            counter_handle.await,
            work_queue.len()
        );
    });

    tracker::announce(&torrent, &id, 6881, tracker::Event::Stopped);

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
    pub length: usize,
}

async fn data_writer(
    mut output: File,
    piece_length: i64,
    mut done_rx: Receiver<DownloadedPiece>,
    num_pieces: usize,
) {
    println!("Starting finished work receiver");
    let mut bitfield = bit_vec::BitVec::from_elem(num_pieces, false);
    loop {
        match done_rx.recv_async().await {
            Ok(p) => {
                output
                    .seek(SeekFrom::Start((p.index * piece_length as usize) as u64))
                    .await
                    .unwrap();
                output.write_all(&p.data).await.unwrap();
                bitfield.set(p.index, true);

                let ones = count_ones(&bitfield);
                let percent = ones as f32 / num_pieces as f32;
                println!("Finished with {:?}. Completed: {} / {} = {}", p.index, ones, num_pieces, percent);
            }
            Err(_) => {
                println!("all senders disconnected");
                break;
            }
        }
    }

    output.sync_all().await.unwrap();
    println!("Done");
}

fn count_ones(bitfield: &bit_vec::BitVec) -> usize {
    bitfield.iter().filter(|&e| e).count()
}
