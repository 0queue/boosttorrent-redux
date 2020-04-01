use std::fmt::Debug;
use std::fmt::Formatter;
use std::time::Duration;

use async_std::sync::Arc;
use crossbeam::queue::SegQueue;
use flume::Sender;

use crate::Piece;

pub struct Peer {
    pub address: usize,
    pub work_queue: Arc<SegQueue<Piece>>,
    pub done_channel: Sender<Piece>,
    pub counter_channel: Sender<usize>,
}

impl Peer {
    pub async fn start(self) -> usize {
        println!("Starting {}", self.address);
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
                async_std::task::sleep(Duration::from_millis(sleep_t as u64)).await
            }

            self.done_channel.send(piece).unwrap();
            self.counter_channel.send(self.address).unwrap();
        }

        self.address.clone()
    }
}

impl Debug for Peer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Peer({:?})", self.address)
    }
}
