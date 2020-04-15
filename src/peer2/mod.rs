use async_std::net::SocketAddrV4;
use async_std::sync::Arc;
use crossbeam::queue::SegQueue;
use flume::{Receiver, RecvError, Sender};

use crate::peer2::protocol::Message;
use crate::PieceMeta;

pub mod spawner;
mod peer;
mod protocol;

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

impl MessageBus {
    fn send(&self, msg: Message) {
        self.tx.send(msg).unwrap()
    }

    async fn recv(&mut self) -> Result<Message, RecvError> {
        self.rx.recv_async().await
    }
}

#[derive(Copy, Clone)]
pub struct Us {
    pub id: [u8; 20],
    pub file_hash: [u8; 20],
}
