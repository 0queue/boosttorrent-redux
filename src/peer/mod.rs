use async_std::net::SocketAddrV4;
use async_std::net::TcpStream;
use async_std::sync::Arc;
use async_std::task::JoinHandle;
use crossbeam::queue::SegQueue;
use flume::Sender;
use futures::AsyncReadExt;

use crate::NUM_PEERS;
pub use crate::peer::process::DownloadedPiece;
use crate::peer::process::InternalId;
use crate::peer::process::PeerProcessor;
use crate::peer::protocol::receiver;
use crate::peer::protocol::sender;
use crate::PieceMeta;

mod process;
mod protocol;

pub async fn async_std_spawner(
    mut addresses: Vec<SocketAddrV4>,
    work_queue: Arc<SegQueue<PieceMeta>>,
    done_channel: Sender<DownloadedPiece>,
    counter_channel: Sender<SocketAddrV4>,
    id: [u8; 20],
    file_hash: [u8; 20]
) {
    let mut active_peers = Vec::new();
    let mut target_num_peers = NUM_PEERS;
    let total_num_pieces = work_queue.len();

    loop {
        while active_peers.len() < target_num_peers {
            if let Some(address) = addresses.pop() {
                let fut = spawn(
                    address,
                    work_queue.clone(),
                    done_channel.clone(),
                    counter_channel.clone(),
                    &id,
                    &file_hash,
                    total_num_pieces,
                );

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
            Result::Err(address) => println!("Peer died {}. Remaining {}", address, addresses.len()),
            Result::Ok(_) => target_num_peers -= 1,
        }
    }

    println!("Done spawning peers");
}

// a little too tall... but how to fix?
async fn spawn(
    address: SocketAddrV4,
    work_queue: Arc<SegQueue<PieceMeta>>,
    done_channel: Sender<DownloadedPiece>,
    counter_channel: Sender<SocketAddrV4>,
    our_id: &[u8; 20],
    file_hash: &[u8; 20],
    total_num_pieces: usize,
) -> Result<JoinHandle<Result<InternalId, InternalId>>, &'static str> {
    println!("{}: attempting to spawn", address);
    let mut stream = match TcpStream::connect(address).await {
        Ok(stream) => stream,
        Err(_) => return Err("Failed to connect"),
    };

    if let Err(s) = protocol::handshake(&mut stream, &our_id, file_hash).await {
        return Err(s);
    }

    let (stream_rx, stream_tx) = stream.split();
    let (send_msg_tx, send_msg_rx) = flume::unbounded();
    let (recv_msg_tx, recv_msg_rx) = flume::unbounded();

    let processor = PeerProcessor::new(
        address,
        work_queue.clone(),
        done_channel.clone(),
        counter_channel.clone(),
        send_msg_tx,
        recv_msg_rx,
        total_num_pieces,
    );

    async_std::task::spawn(sender(stream_tx, send_msg_rx));
    async_std::task::spawn(receiver(stream_rx, recv_msg_tx));
    Ok(async_std::task::spawn(processor.start()))
}
