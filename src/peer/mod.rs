use async_std::net::SocketAddrV4;

pub use protocol::message::Message;

use crate::data::PeerBus;
use crate::data::SharedState;
use crate::peer::peer::Peer;

mod peer;
mod protocol;

/// a task to keep N peers live
pub async fn spawner(
    mut addresses: Vec<SocketAddrV4>,
    peer_bus: PeerBus,
    shared_state: SharedState,
) {
    let mut active_peers = Vec::new();
    let mut target_num_peers = addresses.len();

    loop {
        while active_peers.len() < target_num_peers {
            if let Some(address) = addresses.pop() {
                let num_pieces = shared_state.read().await.total;
                let peer = Peer::new(address, peer_bus.clone(), num_pieces, shared_state.clone());
                active_peers.push(async_std::task::spawn(peer.start()))
            } else {
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
}
