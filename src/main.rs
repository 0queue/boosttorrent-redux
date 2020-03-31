use std::path::PathBuf;

use async_std::sync::Arc;
use crossbeam::queue::SegQueue;
use flume::Receiver;
use flume::Sender;
use futures::StreamExt;
use rand::Rng;
use structopt::StructOpt;

use crate::bencode::de::deserialize;

mod bencode;

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

    let (done_tx, done_rx) = flume::unbounded::<usize>();

    let work_queue: Arc<SegQueue<usize>> = Arc::new(SegQueue::new());
    let (counter_tx, counter_rx) = flume::unbounded::<usize>();

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

    let hashes = Arc::new(torrent
        .get("info")
        .get("pieces")
        .bytes()
        .chunks(20)
        .map(|chunk| {
            let mut hash = [0u8; 20];
            hash.copy_from_slice(chunk);
            hash
        })
        .collect::<Vec<_>>());

    for i in random_indices(hashes.len()) {
        work_queue.push(i)
    }

    println!("Starting processing");
    async_std::task::block_on(async move {
        let mut children = vec![];
        for i in 0..NUM_PEERS {
            children.push(async_std::task::spawn(peer(
                i,
                work_queue.clone(),
                done_tx.clone(),
                counter_tx.clone(),
                hashes.clone(),
            )));
        }

        // seems like senders should be dropped after being distributed
        drop(done_tx);
        drop(counter_tx);

        children.push(async_std::task::spawn(data_writer(done_rx)));
        let counter_handle = async_std::task::spawn(counter(counter_rx));
        futures::future::join_all(children).await;

        println!(
            "Counter: {:?} work_queue.len(): {}",
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

fn random_indices(up_to: usize) -> Vec<usize> {
    use rand::seq::SliceRandom;

    let mut rng = rand::thread_rng();
    let mut indices = (0..up_to).collect::<Vec<_>>();
    indices.shuffle(&mut rng);
    indices
}

async fn peer(
    i: usize,
    q: Arc<SegQueue<usize>>,
    d: Sender<usize>,
    c: Sender<usize>,
    hashes: Arc<Vec<[u8; 20]>>,
) -> () {
    println!("Starting {}", i);
    loop {
        let x = {
            match q.pop() {
                Ok(x) => x,
                _ => {
                    break;
                }
            }
        };
        let sleep_t = (i + 50) * 3;
        println!("{}: Found {:?}. sleeping for {}", i, hashes[x], sleep_t);
        // tweak from_x here to see clear effects on task distribution
        async_std::task::sleep(std::time::Duration::from_millis(sleep_t as u64)).await;
        d.send(x).unwrap();
        c.send(i).unwrap();
    }
    println!("Done {}", i);
}

async fn data_writer(mut done_rx: Receiver<usize>) {
    println!("Starting finished work receiver");
    loop {
        match done_rx.recv_async().await {
            Ok(x) => {
                println!("Finished with {}", x);
            }
            Err(_) => {
                println!("all senders disconnected");
                break;
            }
        }
    }
    println!("Done");
}

async fn counter(counter_rx: Receiver<usize>) -> [u32; NUM_PEERS] {
    let mut res = [0u32; NUM_PEERS];
    counter_rx
        .for_each(|i| {
            res[i] += 1;
            futures::future::ready(())
        })
        .await;
    res
}
