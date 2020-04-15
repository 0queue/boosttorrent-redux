use async_std::net::SocketAddrV4;
use async_std::sync::Arc;
use async_std::sync::RwLock;
use crossbeam::queue::SegQueue;
use flume::Receiver;
use flume::Sender;

pub use writer::writer;

use crate::peer::Message;
use crate::PieceMeta;

mod writer;

pub type SharedState = Arc<RwLock<State>>;

pub struct State {
    pub received: usize,
    pub total: usize,
    pub lifecycle: Lifecycle,
    pub id: [u8; 20],
    pub file_hash: [u8; 20],
}

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
pub struct PeerBus {
    pub work_queue: Arc<SegQueue<PieceMeta>>,
    pub done_tx: Sender<DownloadedPiece>,
    pub counter_tx: Sender<SocketAddrV4>,
}

pub struct MessageBus {
    pub tx: Sender<Message>,
    pub rx: Receiver<Message>,
}
