use std::path::PathBuf;

use async_std::sync::{Arc, Mutex};
use crossbeam::queue::SegQueue;
use flume::Receiver;
use flume::Sender;
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
    println!("announce: {:?}", torrent.get("announce").unwrap().string());

    let (done_tx, done_rx) = flume::unbounded::<i32>();

    let work_queue: Arc<SegQueue<i32>> = Arc::new(SegQueue::new());
    let counter = Arc::new(Mutex::new([0u32; NUM_PEERS]));

    for i in 0..10000 {
        work_queue.push(i)
    }

    println!("Starting processing");
    async_std::task::block_on(async move {
        let mut children = vec![];
        for i in 0..NUM_PEERS {
            let c = counter.clone();
            let d = done_tx.clone();
            children.push(async_std::task::spawn(peer(i, work_queue.clone(), d, c)));
        }

        drop(done_tx);

        children.push(async_std::task::spawn(data_writer(done_rx)));

        futures::future::join_all(children).await;

        println!(
            "Counter: {:?} work_queue.len(): {}",
            counter.lock().await,
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

async fn peer(
    i: usize,
    q: Arc<SegQueue<i32>>,
    d: Sender<i32>,
    c: Arc<Mutex<[u32; NUM_PEERS]>>,
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
        let sleep_t = (i + 1) * 2;
        println!("{}: Found {}. sleeping for {}", i, x, sleep_t);
        // tweak from_x here to see clear effects on task distribution
        async_std::task::sleep(std::time::Duration::from_millis(sleep_t as u64)).await;
        if x.abs() % 2 != (i % 2) as i32 {
            println!("{}: pushing {}", i, x - 1);
            q.push(x - 1);
        } else {
            d.send(x).unwrap();
            c.lock().await[i as usize] += 1;
        }
    }
    println!("Done {}", i);
}

async fn data_writer(mut done_rx: Receiver<i32>) {
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
