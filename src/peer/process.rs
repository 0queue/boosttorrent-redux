use std::cmp::min;
use std::fmt::Debug;
use std::fmt::Formatter;

use async_std::net::SocketAddrV4;
use async_std::sync::Arc;
use bit_vec::BitVec;
use crossbeam::queue::SegQueue;
use flume::Receiver;
use flume::Sender;

use crate::peer::protocol::BlockRequest;
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
    choked: bool,
    interested: bool,
    bitfield: BitVec,
}

impl PeerProcessor {
    pub fn new(
        id: InternalId,
        work_queue: Arc<SegQueue<PieceMeta>>,
        done_tx: Sender<PieceMeta>,
        counter_tx: Sender<InternalId>,
        msg_tx: Sender<Message>,
        msg_rx: Receiver<Message>,
        total_num_pieces: usize,
    ) -> PeerProcessor {
        PeerProcessor {
            id,
            work_queue,
            done_tx,
            counter_tx,
            msg_tx,
            msg_rx,
            choked: false,
            interested: false,
            bitfield: BitVec::with_capacity(total_num_pieces),
        }
    }

    pub async fn start(mut self) -> Result<InternalId, InternalId> {
        println!("Starting {}", self.id);

        self.msg_tx.send(Message::Unchoke).unwrap();
        self.msg_tx.send(Message::Interested).unwrap();

        loop {
            let piece = match self.work_queue.pop() {
                Ok(p) => p,
                Err(_) => break,
            };

            if !self.bitfield[piece.index] {
                self.work_queue.push(piece);
                continue;
            }

            self.download_piece(&piece).await;

            self.done_tx.send(piece).unwrap();
            self.counter_tx.send(self.id).unwrap();
        }

        Ok(self.id)
    }

    async fn download_piece(&mut self, piece: &PieceMeta) {
        let blocks: Vec<BlockRequest> = {
            let mut begin = 0;
            let mut blocks = Vec::new();

            while begin < piece.length {
                blocks.push(BlockRequest {
                    index: piece.index as u32,
                    begin: begin as u32,
                    length: min((piece.length - begin) as u32, BLOCK_LENGTH as u32),
                });

                begin += BLOCK_LENGTH;
            }

            blocks
        };

        // TODO wait to be unchoked, send and receive blocks

        while blocks.len() > 0 {
            let incoming_msgs = self.msg_rx.drain().collect::<Vec<_>>();
            for msg in incoming_msgs {
                match msg {
                    Message::Choke => self.choked = true,
                    Message::Unchoke => self.choked = false,
                    Message::Interested => self.interested = true,
                    Message::NotInterested => self.interested = false,
                    Message::Have(_) => {}
                    Message::Bitfield(b) => {
                        self.bitfield = b;
                    }
                    Message::Request(_) => {}
                    Message::Piece(_) => {}
                    Message::Cancel(_) => {}
                };
            }
        }
    }
}

const BLOCK_LENGTH: usize = 1 << 14; // 2^14

impl Debug for PeerProcessor {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Peer({:?})", self.id)
    }
}
