use std::time::Duration;

use flume::Receiver;
use flume::RecvError;
use flume::Sender;
use flume::TryRecvError;
use util::timer::Timer;

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
    last_sent: Timer,
}

impl MessageBus {
    pub fn new(tx: Sender<Message>, rx: Receiver<Result<Message, ProtocolErr>>) -> MessageBus {
        let mut t = Timer::new();
        t.start();
        MessageBus { tx, rx, last_sent: t }
    }

    pub fn send(&self, msg: Message) {
        self.last_sent.time();
        self.tx.send(msg).unwrap()
    }

    pub async fn recv(&mut self) -> Result<Message, MessageRecvErr> {
        match self.rx.recv_async().await {
            Ok(r) => r.map_err(MessageRecvErr::Protocol),
            Err(e) => Err(MessageRecvErr::Channel(e)),
        }
    }

    pub fn try_recv(&mut self) -> Result<Message, TryMessageRecvErr> {
        match self.rx.try_recv() {
            Ok(r) => r.map_err(TryMessageRecvErr::Protocol),
            Err(e) => Err(TryMessageRecvErr::Channel(e)),
        }
    }

    pub fn last_sent(&self) -> Option<Duration> {
        self.last_sent.time()
    }
}