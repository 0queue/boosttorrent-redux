use async_std::sync::Arc;
use async_std::sync::RwLock;
use crate::PieceMeta;
use bit_vec::BitVec;

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
    pub id: [u8; 20],
    pub file_hash: [u8; 20],
}