use std::cmp::min;
use std::fmt::Debug;
use std::fmt::Formatter;

use async_std::net::SocketAddrV4;
use bit_vec::BitVec;
use futures::stream::FusedStream;
use sha1::Digest;
use sha1::Sha1;

use crate::peer::MessageBus;
use crate::peer::PeerBus;
use crate::peer::protocol::BlockRequest;
use crate::peer::protocol::Message;
use crate::PieceMeta;

pub type InternalId = SocketAddrV4;

pub struct PeerProcessor {
    pub id: InternalId,
    pub bus: PeerBus,
    pub msg_bus: MessageBus,
    choked: bool,
    interested: bool,
    bitfield: BitVec,
}

impl PeerProcessor {
    pub fn new(
        id: InternalId,
        bus: PeerBus,
        message_bus: MessageBus,
        total_num_pieces: usize,
    ) -> PeerProcessor {
        PeerProcessor {
            id,
            bus,
            msg_bus: message_bus,
            choked: true,
            interested: false,
            bitfield: BitVec::with_capacity(total_num_pieces),
        }
    }

    pub async fn start(mut self) -> Result<InternalId, InternalId> {
        println!("{}: Starting", self.id);

        self.msg_bus.send(Message::Interested);

        match self.msg_bus.recv().await {
            Ok(Message::Bitfield(bitfield)) => {
                self.bitfield = bitfield;
            }
            x => {
                eprintln!("{}: Not bitfield: {:?}", self.id, x);
                return Err(self.id);
            }
        }

        loop {
            let piece = match self.bus.work_queue.pop() {
                Ok(p) => p,
                Err(_) => break,
            };

            if !self.bitfield[piece.index] {
                self.bus.work_queue.push(piece);
                async_std::task::yield_now().await;
                continue;
            }

            println!("{}: Attempting download of {}", self.id, piece.index);
            let downloaded = self.download_piece(&piece).await;

            if self.msg_bus.rx.is_terminated() {
                eprintln!("{}: terminated", self.id);
                return Err(self.id);
            }

            match downloaded {
                None => {
                    println!("{}: Failed to get piece {}", self.id, piece.index);
                    self.bus.work_queue.push(piece);
                }
                Some(p) => {
                    println!("{}: SUCCESS piece {}", self.id, piece.index);
                    self.msg_bus.send(Message::Have(piece.index as u32));
                    self.bus.done_tx.send(p).unwrap();
                    self.bus.counter_tx.send(self.id).unwrap();
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

        let total_blocks = blocks.len();
        let mut blocks_received = 0;
        let mut in_flight = 0usize;
        let mut piece_data = vec![0u8; piece.length];

        while blocks.len() > 0 || in_flight > 0 {
            let msg = self.msg_bus.recv().await;
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

            if self.choked {
                // wtf the await should be in the yield_now function??
                async_std::task::yield_now().await;
                continue;
            }

            while in_flight < min(blocks.len(), PIPELINE_LENGTH) && blocks.len() > 0 {
                // fill pipeline
                self.msg_bus.send(Message::Request(blocks.pop().unwrap()));
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
            eprintln!(
                "{}: Mismatched hash total_blocks {}, total received {}",
                self.id, total_blocks, blocks_received
            );
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
