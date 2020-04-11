use std::cmp::min;
use std::fmt::Debug;
use std::fmt::Formatter;

use async_std::net::SocketAddrV4;
use async_std::sync::Arc;
use bit_vec::BitVec;
use crossbeam::queue::SegQueue;
use flume::{Receiver, RecvError};
use flume::Sender;
use futures::stream::FusedStream;
use sha1::Digest;
use sha1::Sha1;

use crate::peer::protocol::BlockRequest;
use crate::peer::protocol::Message;
use crate::PieceMeta;

pub type InternalId = SocketAddrV4;

pub struct PeerProcessor {
    pub id: InternalId,
    pub work_queue: Arc<SegQueue<PieceMeta>>,
    pub done_tx: Sender<DownloadedPiece>,
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
        done_tx: Sender<DownloadedPiece>,
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
        println!("{}: Starting", self.id);

        match self.msg_rx.recv_async().await {
            Ok(Message::Bitfield(bitfield)) => {
                self.bitfield = bitfield;
            }
            x => {
                eprintln!("{}: Not bitfield: {:?}", self.id, x);
                return Err(self.id);
            }
        }

        println!("{}: Sending unchoke and interested", self.id);
        self.msg_tx.send(Message::Unchoke).unwrap();
        self.msg_tx.send(Message::Interested).unwrap();

        loop {
            let piece = match self.work_queue.pop() {
                Ok(p) => p,
                Err(_) => break,
            };

            if !self.bitfield[piece.index] {
                self.work_queue.push(piece);
                async_std::task::yield_now().await;
                continue;
            }

            println!("{}: Attempting download of {}", self.id, piece.index);
            let downloaded = self.download_piece(&piece).await;

            if self.msg_rx.is_terminated() {
                eprintln!("{}: terminated", self.id);
                return Err(self.id);
            }

            match downloaded {
                None => {
                    println!("{}: Failed to get piece {}", self.id, piece.index);
                    self.work_queue.push(piece);
                }
                Some(p) => {
                    println!("{}: SUCCESS piece {}", self.id, piece.index);
                    self.msg_tx.send(Message::Have(p.index as u32)).unwrap();
                    self.done_tx.send(p).unwrap();
                    self.counter_tx.send(self.id).unwrap();
                }
            }
        }

        Ok(self.id)
    }

    async fn download_piece(&mut self, piece: &PieceMeta) -> Option<DownloadedPiece> {
        let mut blocks: Vec<BlockRequest> = {
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

        println!("{}: {} last block: {:?}", self.id, piece.length, blocks[blocks.len() - 1]);

        let total_blocks = blocks.len();
        let mut blocks_received = 0;
        let mut in_flight = 0usize;
        let mut piece_data = vec![0u8; piece.length];

        while blocks.len() > 0 || in_flight > 0 {
            // let incoming_msgs = self.msg_rx.drain().collect::<Vec<_>>();
            // for msg in incoming_msgs {
            // println!("{}: waiting for message", self.id);
            let msg = self.msg_rx.recv_async().await;
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("{}: RecvError {:?}", self.id, e);
                    return None;
                }
            };
            println!("{}: received {}", self.id, &msg);
            match msg {
                Message::Choke => self.choked = true,
                Message::Unchoke => self.choked = false,
                Message::Interested => self.interested = true,
                Message::NotInterested => self.interested = false,
                Message::Have(index) => self.bitfield.set(index as usize, true),
                Message::Bitfield(b) => self.bitfield = b,
                Message::Request(_) => {}
                Message::Piece(response) => {
                    if response.index == piece.index as u32 {
                        // lol I had index not begin here
                        let start = response.begin as usize;
                        let end = start + response.data.len();
                        piece_data[start..end].copy_from_slice(&response.data);
                        in_flight -= 1;
                        blocks_received += 1;
                    }
                }
                Message::Cancel(_) => {}
            }
            // }

            if self.choked {
                // wtf the await should be in the yield_now function??
                async_std::task::yield_now().await;
                continue;
            }

            // println!("{}: Filling pipeline in_flight: {}, blocks.len(): {}", self.id, in_flight, blocks.len());
            while in_flight < min(blocks.len(), PIPELINE_LENGTH) && blocks.len() > 0 {
                // fill pipeline
                self.msg_tx.send(Message::Request(blocks.pop().unwrap())).unwrap();
                in_flight += 1;
            }
        }

        // check sha
        let mut hasher = Sha1::new();
        hasher.input(&piece_data);
        let hash = hasher.result();
        if hash.as_slice() == piece.hash {
            // hooray!
            Some(DownloadedPiece {
                index: piece.index,
                hash: piece.hash,
                data: piece_data,
            })
        } else {
            // oh no
            eprintln!("{}: downloaded hash: {:?}", self.id, hash.as_slice());
            eprintln!("{}: target hash: {:?}", self.id, piece.hash);
            eprintln!("{}: Mismatched hash total_blocks {}, total received {}", self.id, total_blocks, blocks_received);
            None
        }
    }
}

// 2^14
const BLOCK_LENGTH: usize = 1 << 14;
const PIPELINE_LENGTH: usize = 10;

impl Debug for PeerProcessor {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Peer({:?})", self.id)
    }
}

#[derive(Debug)]
pub struct DownloadedPiece {
    pub index: usize,
    pub hash: [u8; 20],
    pub data: Vec<u8>,
}
