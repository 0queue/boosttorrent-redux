use async_std::fs::File;
use async_std::sync::Arc;
use async_std::sync::RwLock;
use bit_vec::BitVec;
use futures::AsyncReadExt;
use futures::AsyncSeekExt;
use futures::AsyncWriteExt;
use futures::io::SeekFrom;
use util::ext::bitvec::BitVecExt;

use crate::counter::Event;
use crate::protocol::BlockRequest;
use crate::protocol::BlockResponse;

#[derive(PartialEq, Copy, Clone)]
#[allow(dead_code)]
pub enum Lifecycle {
    Downloading,
    Endgame,
    Done,
}

#[derive(Debug, Clone)]
pub struct PieceMeta {
    pub index: usize,
    pub hash: [u8; 20],
    pub length: usize,
}

#[derive(Debug)]
pub struct DownloadedPiece {
    pub index: usize,
    pub hash: [u8; 20],
    pub data: Vec<u8>,
}

#[derive(Clone)]
pub struct WorkBus {
    pub tx: async_std::sync::Sender<usize>,
    pub rx: async_std::sync::Receiver<usize>,
}

#[derive(Clone)]
pub struct ControllerBus {
    pub work_bus: WorkBus,
    pub done_tx: flume::Sender<DownloadedPiece>,
    pub counter_tx: flume::Sender<Event>,
}

pub type ControllerState = Arc<RwLock<State>>;

pub struct State {
    pub haves: Vec<usize>,
    pub bitfield: BitVec,
    pub lifecycle: Lifecycle,
    file: File,
    piece_length: usize,
}

impl State {
    pub fn new(num_pieces: usize, piece_length: usize, file: File) -> State {
        State {
            haves: vec![],
            bitfield: BitVec::from_elem(num_pieces, false),
            lifecycle: Lifecycle::Downloading,
            file,
            piece_length,
        }
    }

    pub fn take_file(self) -> File {
        self.file
    }

    pub async fn try_read_request(&mut self, block_request: &BlockRequest) -> Option<BlockResponse> {
        // let mut buf = vec![0u8; block_request.length as usize];
        let mut block_response = BlockResponse {
            index: block_request.index,
            begin: block_request.begin,
            data: vec![0u8; block_request.length as usize],
        };

        if !self.bitfield.get(block_request.index as usize).unwrap_or(false) {
            return None;
        }

        let start = (block_request.index * self.piece_length as u32 + block_request.begin) as u64;
        self.file.seek(SeekFrom::Start(start)).await.unwrap();
        self.file.read_exact(&mut block_response.data).await.unwrap();
        Some(block_response)
    }
}

pub struct TorrentInfo {
    pub pieces: Vec<PieceMeta>,
    pub piece_length: usize,
    pub id: [u8; 20],
    pub file_hash: [u8; 20],
}

pub async fn controller(
    mut output_file: File,
    torrent_info: Arc<TorrentInfo>,
    mut done_rx: flume::Receiver<DownloadedPiece>,
    work_rx: async_std::sync::Receiver<usize>,
    controller_state: ControllerState,
) -> File {
    let mut local_bitfield = controller_state.read().await.bitfield.clone();

    loop {
        match done_rx.recv_async().await {
            Err(_) => {
                println!("all senders disconnected");
                break;
            }
            Ok(piece) => if !local_bitfield[piece.index] {
                // write to file
                output_file
                    .seek(SeekFrom::Start((piece.index * torrent_info.piece_length) as u64))
                    .await
                    .unwrap();
                output_file.write_all(&piece.data).await.unwrap();

                // update state
                {
                    local_bitfield.set(piece.index, true);
                    let mut write = controller_state.write().await;
                    write.bitfield.set(piece.index, true);
                    write.haves.push(piece.index);
                    if write.bitfield.all() {
                        write.lifecycle = Lifecycle::Done;
                    } else if work_rx.len() < 10 && write.lifecycle == Lifecycle::Downloading {
                        write.lifecycle = Lifecycle::Endgame;
                        println!("ENDGAME TRIGGERED");
                        println!("> work_rx.len(): {}", work_rx.len());
                        println!("> zeroes: {:?}", write.bitfield.zeroes());
                    }
                }

                // report
                let ones = local_bitfield.ones().len();
                let percent = ones as f32 / torrent_info.pieces.len() as f32;
                println!(
                    "Finished with {:?}. Completed: {} / {} = {:.2}%",
                    piece.index, ones, torrent_info.pieces.len(), percent * 100f32
                )
            },
        }
    }

    println!("Done.  Zeroes: {:?}", local_bitfield.zeroes());

    output_file
}