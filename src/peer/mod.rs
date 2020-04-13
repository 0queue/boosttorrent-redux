use std::time::Duration;

use async_std::net::SocketAddrV4;
use async_std::net::TcpStream;
use async_std::sync::Arc;
use async_std::task::JoinHandle;
use crossbeam::queue::SegQueue;
use flume::Receiver;
use flume::RecvError;
use flume::Sender;
use futures::AsyncReadExt;

use crate::NUM_PEERS;
pub use crate::peer::process::DownloadedPiece;
use crate::peer::process::InternalId;
use crate::peer::process::PeerProcessor;
use crate::peer::protocol::Message;
use crate::peer::protocol::receiver;
use crate::peer::protocol::sender;
use crate::PieceMeta;

mod process;
mod protocol;
mod handshake;

#[derive(Clone)]
pub struct PeerBus {
    pub work_queue: Arc<SegQueue<PieceMeta>>,
    pub done_tx: Sender<DownloadedPiece>,
    pub counter_tx: Sender<SocketAddrV4>,
    // not working as expected, I guess I don't know how channels work
    pub have_tx: crossbeam::Sender<u32>,
    pub have_rx: crossbeam::Receiver<u32>
}

pub struct MessageBus {
    pub tx: Sender<Message>,
    pub rx: Receiver<Message>,
}

impl MessageBus {
    fn send (&self, msg: Message) {
        self.tx.send(msg).unwrap()
    }

    async fn recv(&mut self) -> Result<Message, RecvError> {
        self.rx.recv_async().await
    }
}

pub struct Us {
    pub id: [u8; 20],
    pub file_hash: [u8; 20],
}

pub async fn async_std_spawner(mut addresses: Vec<SocketAddrV4>, bus: PeerBus, us: Us) {
    let mut active_peers = Vec::new();
    let mut target_num_peers = NUM_PEERS;
    let total_num_pieces = bus.work_queue.len();

    loop {
        while active_peers.len() < target_num_peers {
            if let Some(address) = addresses.pop() {
                let fut = spawn(address, bus.clone(), &us, total_num_pieces);

                match fut.await {
                    Ok(handle) => {
                        println!("{}: Spawned", address);
                        active_peers.push(handle);
                    }
                    Err(msg) => println!("{}: Error spawning {}", address, msg),
                }
            } else {
                // no more address to try
                break;
            }
        }

        if active_peers.len() == 0 {
            break;
        }

        let (res, _, others) = futures::future::select_all(active_peers).await;
        active_peers = others;
        match res {
            Result::Err(address) => println!(
                "Peer died {}. Active {}. Remaining {}",
                address,
                active_peers.len(),
                addresses.len()
            ),
            Result::Ok(_) => target_num_peers -= 1,
        }
    }

    println!("Done spawning peers");
}

// a little too tall... but how to fix?
async fn spawn(
    address: SocketAddrV4,
    bus: PeerBus,
    us: &Us,
    total_num_pieces: usize,
) -> Result<JoinHandle<Result<InternalId, InternalId>>, &'static str> {
    println!("{}: attempting to spawn", address);
    let mut stream = match async_std::io::timeout(Duration::from_secs(30), TcpStream::connect(address)).await {
        Ok(stream) => stream,
        Err(_) => return Err("Failed to connect"),
    };

    if let Err(s) = handshake::handshake(&mut stream, us).await {
        return Err(s);
    }

    let (stream_rx, stream_tx) = stream.split();
    let (send_msg_tx, send_msg_rx) = flume::unbounded();
    let (recv_msg_tx, recv_msg_rx) = flume::unbounded();

    let message_bus = MessageBus {
        tx: send_msg_tx,
        rx: recv_msg_rx,
    };

    let processor = PeerProcessor::new(address, bus, message_bus, total_num_pieces);

    async_std::task::spawn(sender(stream_tx, send_msg_rx));
    async_std::task::spawn(receiver(stream_rx, recv_msg_tx));
    Ok(async_std::task::spawn(processor.start()))
}
