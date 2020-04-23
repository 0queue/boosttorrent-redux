use async_std::sync::Arc;
use async_std::sync::RwLock;
use crate::PieceMeta;
use bit_vec::BitVec;
use async_std::fs::File;
use flume::RecvError;
use util::ext::bitvec::BitVecExt;
use futures::{AsyncSeekExt, AsyncWriteExt};
use futures::io::SeekFrom;

#[derive(PartialEq, Copy, Clone)]
#[allow(dead_code)]
pub enum Lifecycle {
    Downloading,
    Endgame,
    Done,
}

#[derive(Debug)]
pub struct DownloadedPiece {
    pub index: usize,
    pub hash: [u8; 20],
    pub data: Vec<u8>,
}

#[derive(Clone)]
pub struct WorkBus {
    pub work_tx: async_std::sync::Sender<usize>,
    pub work_rx: async_std::sync::Receiver<usize>,
}

#[derive(Clone)]
pub struct ControllerBus {
    pub work_bus: WorkBus,
    pub done_tx: flume::Sender<DownloadedPiece>,
    pub counter_tx: flume::Sender<DownloadedPiece>,
}

pub type ControllerState = Arc<RwLock<State>>;

pub struct State {
    pub haves: Vec<usize>,
    pub bitfield: BitVec,
    pub lifecycle: Lifecycle,
}

pub struct TorrentInfo {
    pub pieces: Vec<PieceMeta>,
    pub piece_length: usize,
    pub id: [u8; 20],
    pub file_hash: [u8; 20],
}

pub async fn controller(
    mut output_file: File,
    torrent_info: TorrentInfo,
    mut done_rx: flume::Receiver<DownloadedPiece>,
    work_rx: async_std::sync::Receiver<usize>,
    controller_state: ControllerState,
) {
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
}