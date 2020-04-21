use async_std::sync::Arc;
use async_std::sync::RwLock;
use flume::RecvError;
use flume::Receiver;
use flume::TryRecvError;
use flume::Sender;

pub use writer::writer;

use crate::broadcast;
use crate::peer::Message;
use crate::PieceMeta;
use crate::counter::Event;

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
    pub work_tx: async_std::sync::Sender<usize>,
    pub work_rx: async_std::sync::Receiver<usize>,
    pub done_tx: Sender<DownloadedPiece>,
    pub counter_tx: Sender<Event>,
    pub endgame_rx: broadcast::Receiver<usize>,
    pub haves: Arc<RwLock<Vec<usize>>>,
    pub pieces: Arc<Vec<PieceMeta>>
}

pub struct MessageBus {
    pub tx: Sender<Message>,
    pub rx: Receiver<Message>,
}

impl MessageBus {
    pub fn send(&self, msg: Message) {
        self.tx.send(msg).unwrap()
    }

    pub async fn recv(&mut self) -> Result<Message, RecvError> {
        self.rx.recv_async().await
    }

    pub fn try_recv(&mut self) -> Result<Message, TryRecvError> {
        self.rx.try_recv()
    }
}
