use std::path::PathBuf;

use rand::Rng;
use structopt::StructOpt;

use crate::bencode::de::deserialize;
use async_std::sync::{Arc, Mutex};
use crossbeam::queue::SegQueue;
use flume::Sender;
use lazy_static::lazy_static;

mod bencode;

#[derive(Debug, StructOpt)]
#[structopt()]
struct Args {
    #[structopt(parse(from_os_str))]
    torrent_file_path: PathBuf,
}

lazy_static! {
    static ref WORK_QUEUE: SegQueue<i32> = SegQueue::new();
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Args = Args::from_args();
    println!("{:?}", args);

    let contents = std::fs::read(args.torrent_file_path)?;

    let torrent = deserialize(&contents)?;

    let id = gen_peer_id();
    println!("This is {:02x?}", id);
    println!("announce: {:?}", torrent.get("announce").unwrap().string());

    async_std::task::block_on(async {
        println!("woah async");
        async_std::task::spawn(async {
            println!("time to nest");
        })
            .await;
    });

    let (done_tx, mut done_rx) = flume::unbounded::<i32>();

    async_std::task::block_on(async {
        for i in 0..10000 {
            WORK_QUEUE.push(i)
        }
    });

    let counter: async_std::sync::Arc<async_std::sync::Mutex<[u32; 5]>> =
        async_std::sync::Arc::new(async_std::sync::Mutex::new([0u32; 5]));

    println!("Starting processing");
    async_std::task::block_on(async move {
        let mut children = vec![];
        for i in 0..5i32 {
            let c = counter.clone();
            let d = done_tx.clone();
            children.push(async_std::task::spawn(peer(i, &WORK_QUEUE, d, c)));
        }

        drop(done_tx);

        children.push(async_std::task::spawn(async move {
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
        }));

        futures::future::join_all(children).await;

        println!("Counter: {:?} WORK_QUEUE.len(): {}", counter.lock().await, WORK_QUEUE.len());
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
    i: i32,
    q: &SegQueue<i32>,
    d: Sender<i32>,
    c: Arc<Mutex<[u32; 5]>>,
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
        if x.abs() % 2 != i % 2 {
            println!("{}: pushing {}", i, x - 1);
            q.push(x - 1);
        } else {
            d.send(x).unwrap();
            c.lock().await[i as usize] += 1;
        }
    }
    println!("Done {}", i);
}
