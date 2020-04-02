use std::fmt::Debug;
use std::fmt::Formatter;
use std::time::Duration;

use async_std::sync::Arc;
use crossbeam::queue::SegQueue;
use flume::Sender;

use crate::{NUM_PEERS, Piece};

pub struct Peer {
    pub address: usize,
    pub work_queue: Arc<SegQueue<Piece>>,
    pub done_channel: Sender<Piece>,
    pub counter_channel: Sender<usize>,
}

pub enum Result {
    Done,
    Dead(usize),
}

impl Peer {
    pub async fn start(self) -> Result {
        println!("Starting {}", self.address);
        let mut self_counter = 0;
        loop {
            let piece = match self.work_queue.pop() {
                Ok(p) => p,
                Err(_) => break,
            };

            {
                // fake work
                let sleep_t = (self.address + 25) * 3;
                println!(
                    "{}: Found {:?}. sleeping for {}",
                    self.address, piece.hash, sleep_t
                );
                async_std::task::sleep(Duration::from_millis(sleep_t as u64)).await;

                if self_counter >= 250 {
                    // pretend to die
                    self.work_queue.push(piece);
                    return Result::Dead(self.address)
                }

            }

            self.done_channel.send(piece).unwrap();
            self.counter_channel.send(self.address).unwrap();
            self_counter += 1;
        }

        Result::Done
    }
}

impl Debug for Peer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Peer({:?})", self.address)
    }
}


// not sure how to disconnect this from async_std

pub async fn async_std_spawner(
    mut addresses: Vec<usize>,
    work_queue: Arc<SegQueue<Piece>>,
    done_channel: Sender<Piece>,
    counter_channel: Sender<usize>,
) {
    let new = |address: usize| -> Peer {
        Peer {
            address,
            work_queue: work_queue.clone(),
            done_channel: done_channel.clone(),
            counter_channel: counter_channel.clone(),
        }
    };

    let mut active_peers = Vec::new();

    // initial set of peers
    for _ in 0..NUM_PEERS {
        if let Some(address) = addresses.pop() {
            active_peers.push(async_std::task::spawn(new(address).start()))
        }
    }

    while active_peers.len() > 0 {
        let (res, _, others) = futures::future::select_all(active_peers).await;
        active_peers = others;
        if let Result::Dead(address) = res {
            println!("Peer died: {}", address);
            if let Some(address) = addresses.pop() {
                active_peers.push(async_std::task::spawn(new(address).start()))
            }
        }
    }

    // TODO if the work queue is not empty there are no more active peers...
    //   talk to the tracker and/or return a negative result

    println!("Done spawning peers");
}