use flume::Receiver;
use flume::RecvError;
use flume::Sender;
use flume::TryRecvError;

use crate::protocol::message::Message;
use crate::protocol::ProtocolErr;

pub enum MessageRecvErr {
    Protocol(ProtocolErr),
    Channel(RecvError),
}

pub enum TryMessageRecvErr {
    Protocol(ProtocolErr),
    Channel(TryRecvError),
}


pub struct MessageBus {
    tx: Sender<Message>,
    rx: Receiver<Result<Message, ProtocolErr>>,
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