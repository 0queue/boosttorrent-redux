use async_std::net::TcpStream;
use flume::Receiver;
use flume::Sender;
use futures::io::ReadHalf;
use futures::io::WriteHalf;

pub use handshake::handshake;

use crate::protocol::message::Message;

mod handshake;
pub mod message;

#[derive(Debug, Clone)]
pub struct BlockRequest {
    pub index: u32,
    pub begin: u32,
    pub length: u32,
}

#[derive(Debug)]
pub struct BlockResponse {
    pub index: u32,
    pub begin: u32,
    pub data: Vec<u8>,
}

pub enum ProtocolErr {}

pub async fn sender(mut stream: WriteHalf<TcpStream>, mut msg_rx: Receiver<Message>) {
    loop {
        let msg = match msg_rx.recv_async().await {
            Ok(m) => m,
            Err(_) => return,
        };

        if msg.send(&mut stream).await.is_err() {
            return;
        }
    }
}

pub async fn receiver(mut stream: ReadHalf<TcpStream>, msg_tx: Sender<Result<Message, ProtocolErr>>) {
    loop {
        let msg = match Message::from(&mut stream).await {
            Ok(m) => m,
            Err(_) => return, // TODO send a protocol error
        };

        if msg_tx.send(Ok(msg)).is_err() {
            return;
        }
    }
}
