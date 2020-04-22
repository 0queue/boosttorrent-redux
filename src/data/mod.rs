use async_std::sync::Arc;
use async_std::sync::RwLock;
use flume::Receiver;
use flume::RecvError;
use flume::Sender;
use flume::TryRecvError;

pub use writer::writer;

use crate::broadcast;
use crate::counter::Event;
use crate::PieceMeta;
use crate::protocol::message::Message;
use crate::protocol::ProtocolErr;

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
    pub pieces: Arc<Vec<PieceMeta>>,
}

pub struct MessageBus {
    tx: Sender<Message>,
    rx: Receiver<Result<Message, ProtocolErr>>,
}

pub enum MessageRecvErr {
    Protocol(ProtocolErr),
    Channel(RecvError),
}

pub enum TryMessageRecvErr {
    Protocol(ProtocolErr),
    Channel(TryRecvError),
}

impl MessageBus {
    pub fn new(tx: Sender<Message>, rx: Receiver<Result<Message, ProtocolErr>>) -> MessageBus {
        MessageBus { tx, rx }
    }

    pub fn send(&self, msg: Message) {
        self.tx.send(msg).unwrap()
    }

    pub async fn recv(&mut self) -> Result<Message, MessageRecvErr> {
        match self.rx.recv_async().await {
            Ok(r) => r.map_err(|e| MessageRecvErr::Protocol(e)),
            Err(e) => Err(MessageRecvErr::Channel(e)),
        }
    }

    pub fn try_recv(&mut self) -> Result<Message, TryMessageRecvErr> {
        match self.rx.try_recv() {
            Ok(r) => r.map_err(|e| TryMessageRecvErr::Protocol(e)),
            Err(e) => Err(TryMessageRecvErr::Channel(e)),
        }
    }
}
