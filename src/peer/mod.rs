use async_std::net::SocketAddrV4;

pub use protocol::message::Message;

use crate::data::PeerBus;
use crate::data::SharedState;
use crate::data::Us;
use crate::MAX_PEERS;
use crate::peer::peer::Peer;
use crate::PieceMeta;

mod peer;
mod protocol;

// use crate::peer::PeerBus;
// use crate::peer::Us;

/// a task to keep N peers live
pub async fn spawner(
    us: Us,
    mut addresses: Vec<SocketAddrV4>,
    peer_bus: PeerBus,
    shared_state: SharedState,
    endgame_pieces: Vec<PieceMeta>,
) {
    let mut active_peers = Vec::new();
    let mut target_num_peers = MAX_PEERS;
    let num_pieces = peer_bus.work_queue.len() + endgame_pieces.len();

    loop {
        while active_peers.len() < target_num_peers {
            if let Some(address) = addresses.pop() {
                let peer = Peer::new(address, peer_bus.clone(), num_pieces);
                active_peers.push(async_std::task::spawn(peer.start(
                    us,
                    shared_state.clone(),
                    endgame_pieces.clone(),
                )))
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
