use std::cmp::min;
use std::path::PathBuf;

use async_std::fs::OpenOptions;
use async_std::net::Ipv4Addr;
use async_std::net::SocketAddrV4;
use async_std::path::Path;
use async_std::sync::Arc;
use async_std::sync::RwLock;
use bencode::BVal;
use byteorder::BigEndian;
use byteorder::ByteOrder;
use futures::AsyncReadExt;
use futures::AsyncSeekExt;
use futures::io::SeekFrom;
use md5::Digest;
use md5::Md5;
use rand::Rng;
use structopt::StructOpt;
use util::ext::duration::DurationExt;
use util::timer::Timer;

use crate::controller::controller;
use crate::controller::ControllerBus;
use crate::controller::PieceMeta;
use crate::controller::State;
use crate::controller::TorrentInfo;
use crate::controller::WorkBus;
use crate::counter::Counter;
use std::sync::atomic::{AtomicBool, Ordering};

mod counter;
mod tracker;
mod protocol;
mod controller;
mod peer;

#[derive(Debug, StructOpt)]
#[structopt()]
pub struct Args {
    #[structopt(parse(from_os_str))]
    pub torrent_file_path: PathBuf,

    #[structopt(long)]
    pub md5sum: Option<String>,
} // TODO remove pub


// TODO small features to add:
//  - colored output (better logs in general)
//  - proper exit codes

// TODO larger features:
//  - bitfield broadcasting
//  - connection listener/keep alive on download finish
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let running_flag = Arc::new(AtomicBool::new(true));
    {
        let r = running_flag.clone();
        ctrlc::set_handler(move || {
            r.store(false, Ordering::SeqCst);
        }).unwrap();
    }

    let args: Args = Args::from_args();
    let mut timer = Timer::new();
    timer.start();

    let torrent = {
        let contents = std::fs::read(args.torrent_file_path.clone())?;
        bencode::de::deserialize(&contents)?
    };

    let (output, input) = async_std::task::block_on(async {
        let name = torrent["info"]["name"].string();
        if Path::new(&name).exists().await {
            println!("File already exists, overwriting: {}", name);
        }

        let output = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(name.clone())
            .await
            .unwrap();

        let input = OpenOptions::new()
            .read(true)
            .open(name)
            .await
            .unwrap();

        (output, input)
    });

    let md5sum = args.md5sum.as_ref().map(|s| hex::decode(s).unwrap());

    let torrent_info = {
        let piece_length = torrent["info"]["piece length"].integer() as usize;
        let file_length = torrent["info"]["length"].integer();

        let pieces = torrent["info"]["pieces"]
            .bytes()
            .chunks_exact(20)
            .enumerate()
            .map(|(index, chunk)| {
                let mut hash = [0u8; 20];
                hash.copy_from_slice(chunk);
                let length = {
                    let start = index * piece_length;
                    let end = min(start + piece_length, file_length as usize);
                    end - start
                };

                PieceMeta { index, hash, length }
            });

        Arc::new(TorrentInfo {
            pieces: pieces.collect(),
            piece_length,
            id: gen_peer_id(),
            file_hash: torrent["info"].hash(),
        })
    };
    let torrent_info_clone = torrent_info.clone();

    let addresses = {
        let response = tracker::announce(&torrent, &torrent_info.id, 6881, tracker::Event::Started);

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

    let work = {
        use rand::seq::SliceRandom;

        let mut rng = rand::thread_rng();
        let mut indices = (0..torrent_info.pieces.len()).collect::<Vec<_>>();
        indices.shuffle(&mut rng);
        indices
    };

    // channel time
    let (work_tx, work_rx) = async_std::sync::channel(torrent_info.pieces.len());
    let (done_tx, done_rx) = flume::unbounded();
    let (counter, counter_tx) = Counter::new();

    let (downloaded, success) = async_std::task::block_on(async move {
        for i in work {
            work_tx.send(i).await;
        }

        let controller_bus = ControllerBus {
            work_bus: WorkBus { tx: work_tx, rx: work_rx.clone() },
            done_tx,
            counter_tx,
        };

        let controller_state = Arc::new(RwLock::new(State::new(
            torrent_info.pieces.len(),
            torrent_info.piece_length,
            input,
        )));

        let spawner_handle = async_std::task::spawn(peer::spawner(
            addresses,
            torrent_info.clone(),
            running_flag.clone(),
            controller_bus,
            controller_state.clone(),
        ));

        let controller_handle = async_std::task::spawn(controller(
            output,
            torrent_info.clone(),
            done_rx,
            work_rx.clone(),
            controller_state.clone(),
        ));

        let counter_handle = async_std::task::spawn(counter.start());

        spawner_handle.await;
        let mut output = controller_handle.await;
        let counted = counter_handle.await;

        let success = if let Some(hash) = &md5sum {
            let mut hasher = Md5::new();
            let mut buf = vec![0u8; 1_000_000];
            output.seek(SeekFrom::Start(0)).await.unwrap();

            println!("Checking md5sum...");
            let mut input = Arc::try_unwrap(controller_state).ok().unwrap().into_inner().take_file();
            let file_hash = loop {
                let read = input.read(&mut buf).await.unwrap();
                if read == 0 {
                    break hasher.result().to_vec();
                }

                hasher.input(&buf[0..read]);
            };

            if hash == &file_hash {
                true
            } else {
                eprintln!("Expected {}", args.md5sum.unwrap());
                eprintln!("Found    {}", hex::encode(&file_hash));
                false
            }
        } else {
            true
        };

        println!("Counter: {:#?}", counted);
        output.sync_all().await.unwrap();

        if success {
            if md5sum.is_some() {
                println!("md5sum matches!");
            } else {
                println!("Success!");
            }
        } else {
            eprintln!("failed, please check logs")
        }

        (output.metadata().await.unwrap().len(), success)
    });

    tracker::announce(&torrent, &torrent_info_clone.id, 6881, tracker::Event::Stopped(downloaded as usize));

    println!("Total time: {}", timer.time().unwrap().time_fmt());

    if success {
        Ok(())
    } else {
        Err("Failure".into())
    }
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