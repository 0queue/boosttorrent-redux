use async_std::net::SocketAddrV4;
use async_std::net::TcpStream;
use async_std::sync::Arc;
use async_std::task::JoinHandle;
use crossbeam::queue::SegQueue;
use flume::Sender;
use futures::AsyncReadExt;

use crate::NUM_PEERS;
use crate::peer::process::InternalId;
use crate::peer::process::PeerProcessor;
use crate::PieceMeta;

mod process;
mod protocol;
mod recv;
mod send;

// not sure how to disconnect this from async_std
pub async fn async_std_spawner(
    mut addresses: Vec<SocketAddrV4>,
    work_queue: Arc<SegQueue<PieceMeta>>,
    done_channel: Sender<PieceMeta>,
    counter_channel: Sender<SocketAddrV4>,
    id: [u8; 20],
) {
    let mut active_peers = Vec::new();

    // TODO this doesn't actually handle connections that fail to start very well

    // initial set of peers
    for _ in 0..NUM_PEERS {
        if let Some(address) = addresses.pop() {
            if let Some(handle) = spawn(
                address,
                work_queue.clone(),
                done_channel.clone(),
                counter_channel.clone(),
                &id,
                &[0u8; 20],
            )
                .await
            {
                active_peers.push(handle);
            }
        }
    }

    while active_peers.len() > 0 {
        let (res, _, others) = futures::future::select_all(active_peers).await;
        active_peers = others;
        if let Result::Err(address) = res {
            println!("Peer died: {}", address);
            if let Some(address) = addresses.pop() {
                if let Some(handle) = spawn(
                    address,
                    work_queue.clone(),
                    done_channel.clone(),
                    counter_channel.clone(),
                    &id,
                    &[0u8; 20],
                )
                    .await
                {
                    active_peers.push(handle);
                }
            }
        }
    }

    // TODO if the work queue is not empty there are no more active peers...
    //   talk to the tracker and/or return a negative result

    println!("Done spawning peers");
}

// a little too tall... but how to fix?
async fn spawn(
    address: SocketAddrV4,
    work_queue: Arc<SegQueue<PieceMeta>>,
    done_channel: Sender<PieceMeta>,
    counter_channel: Sender<SocketAddrV4>,
    our_id: &[u8; 20],
    file_hash: &[u8; 20],
) -> Option<JoinHandle<Result<InternalId, InternalId>>> {
    let mut stream = match TcpStream::connect(address).await {
        Ok(stream) => stream,
        Err(_) => return None,
    };

    if let Err(_) = protocol::handshake(&mut stream, &our_id, file_hash).await {
        return None;
    }

    let (stream_rx, stream_tx) = stream.split();
    let (send_msg_tx, send_msg_rx) = flume::unbounded();
    let (recv_msg_tx, recv_msg_rx) = flume::unbounded();

    let processor = PeerProcessor {
        id: address,
        work_queue: work_queue.clone(),
        done_tx: done_channel.clone(),
        counter_tx: counter_channel.clone(),
        msg_tx: send_msg_tx,
        msg_rx: recv_msg_rx,
    };

    async_std::task::spawn(send::sender(stream_tx, send_msg_rx));
    async_std::task::spawn(recv::receiver(stream_rx, recv_msg_tx));
    Some(async_std::task::spawn(processor.start()))
}
