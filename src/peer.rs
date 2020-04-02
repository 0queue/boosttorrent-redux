use std::fmt::Debug;
use std::fmt::Formatter;
use std::time::Duration;

use async_std::io::Error;
use async_std::net::SocketAddrV4;
use async_std::net::TcpStream;
use async_std::sync::Arc;
use crossbeam::queue::SegQueue;
use flume::Sender;
use futures::{AsyncWriteExt, TryFutureExt};

use crate::NUM_PEERS;
use crate::PieceMeta;
use crate::protocol::handshake;

pub struct Peer {
    pub address: SocketAddrV4,
    pub work_queue: Arc<SegQueue<PieceMeta>>,
    pub done_channel: Sender<PieceMeta>,
    pub counter_channel: Sender<SocketAddrV4>,
}

impl Peer {
    pub async fn start(self, id: [u8; 20]) -> Result<(), SocketAddrV4> {
        println!("Starting {}", self.address);
        let mut self_counter = 0i32;

        let mut stream = TcpStream::connect(self.address)
            .await
            .map_err(|_| self.address)?;

        handshake(&mut stream, &[0u8; 20], &id)
            .await
            .map_err(|_| self.address)?;

        loop {
            let piece = match self.work_queue.pop() {
                Ok(p) => p,
                Err(_) => break,
            };

            {
                // fake work
                let sleep_t = 30;
                println!(
                    "{}: Found {:?}. sleeping for {}",
                    self.address, piece.hash, sleep_t
                );
                async_std::task::sleep(Duration::from_millis(sleep_t as u64)).await;

                if self_counter >= 250 {
                    // pretend to die
                    self.work_queue.push(piece);
                    return Result::Err(self.address);
                }
            }

            self.done_channel.send(piece).unwrap();
            self.counter_channel.send(self.address).unwrap();
            self_counter += 1;
        }

        Result::Ok(())
    }
}

impl Debug for Peer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Peer({:?})", self.address)
    }
}

// not sure how to disconnect this from async_std
pub async fn async_std_spawner(
    mut addresses: Vec<SocketAddrV4>,
    work_queue: Arc<SegQueue<PieceMeta>>,
    done_channel: Sender<PieceMeta>,
    counter_channel: Sender<SocketAddrV4>,
    id: [u8; 20],
) {
    let new = |address: SocketAddrV4| -> Peer {
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
            active_peers.push(async_std::task::spawn(new(address).start(id.clone())))
        }
    }

    while active_peers.len() > 0 {
        let (res, _, others) = futures::future::select_all(active_peers).await;
        active_peers = others;
        if let Result::Err(address) = res {
            println!("Peer died: {}", address);
            if let Some(address) = addresses.pop() {
                active_peers.push(async_std::task::spawn(new(address).start(id.clone())))
            }
        }
    }

    // TODO if the work queue is not empty there are no more active peers...
    //   talk to the tracker and/or return a negative result

    println!("Done spawning peers");
}
