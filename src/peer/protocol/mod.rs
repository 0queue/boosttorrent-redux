use async_std::net::TcpStream;
use flume::Receiver;
use flume::Sender;
use futures::io::ReadHalf;
use futures::io::WriteHalf;

pub use handshake::handshake;

use crate::peer::protocol::message::Message;

mod handshake;
pub mod message;

#[derive(Debug)]
pub struct BlockRequest {
    pub index: u32,
    pub begin: u32,
    pub length: u32,
}

impl From<(u32, u32, u32)> for BlockRequest {
    fn from((index, begin, length): (u32, u32, u32)) -> Self {
        BlockRequest {
            index,
            begin,
            length,
        }
    }
}

#[derive(Debug)]
pub struct BlockResponse {
    pub index: u32,
    pub begin: u32,
    pub data: Vec<u8>,
}

pub async fn sender(mut stream: WriteHalf<TcpStream>, mut msg_rx: Receiver<Message>) {
    loop {
        let msg = match msg_rx.recv_async().await {
            Ok(m) => m,
            Err(_) => return,
        };

        if let Err(_) = msg.send(&mut stream).await {
            return;
        }
    }
}

pub async fn receiver(mut stream: ReadHalf<TcpStream>, msg_tx: Sender<Message>) {
    loop {
        let msg = match Message::from(&mut stream).await {
            Ok(m) => m,
            Err(s) => {
                eprintln!("error receiving: {}", s);
                return;
            }
        };

        if let Err(_) = msg_tx.send(msg) {
            return;
        }
    }
}
