use std::fmt::{Debug, Formatter};
use std::time::Duration;

use async_std::net::SocketAddrV4;
use async_std::sync::Arc;
use crossbeam::queue::SegQueue;
use flume::Receiver;
use flume::Sender;
use futures::stream::FusedStream;

use crate::peer::protocol::Message;
use crate::PieceMeta;

pub type InternalId = SocketAddrV4;

pub struct PeerProcessor {
    pub id: InternalId,
    pub work_queue: Arc<SegQueue<PieceMeta>>,
    pub done_tx: Sender<PieceMeta>,
    pub counter_tx: Sender<InternalId>,
    pub msg_tx: Sender<Message>,
    pub msg_rx: Receiver<Message>,
}

impl PeerProcessor {
    pub async fn start(self) -> Result<InternalId, InternalId> {
        println!("Starting {}", self.id);
        let mut self_counter = 0u32;

        loop {
            let piece = match self.work_queue.pop() {
                Ok(p) => p,
                Err(_) => break,
            };

            {
                // fake work
                let sleep_t = 30;
                println!(
                    "{}: Found {:?} sleeping for {}",
                    self.id, piece.hash, sleep_t
                );
                async_std::task::sleep(Duration::from_millis(sleep_t)).await;

                if self_counter >= 250 || self.msg_rx.is_terminated() {
                    // pretend to die
                    self.work_queue.push(piece);
                    return Err(self.id);
                }
            }

            self.done_tx.send(piece).unwrap();
            self.counter_tx.send(self.id).unwrap();
            self_counter += 1;
        }

        Ok(self.id)
    }
}

impl Debug for PeerProcessor {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Peer({:?})", self.id)
    }
}
