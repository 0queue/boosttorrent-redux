use async_std::net::SocketAddrV4;

use crate::data::PeerBus;
use crate::data::SharedState;
use crate::peer::peer::Peer;
use util::ext::duration::DurationExt;

mod peer;
mod message_bus;

pub async fn spawner(
    addresses: Vec<SocketAddrV4>,
    peer_bus: PeerBus,
    shared_state: SharedState,
) {
    let mut active_peers = Vec::new();
    let num_pieces = shared_state.read().await.total;

    for address in addresses {
        let peer = Peer::new(address, peer_bus.clone(), num_pieces, shared_state.clone());
        active_peers.push(async_std::task::spawn(peer.start()))
    }

    while active_peers.len() > 0 {
        let (res, _, others) = futures::future::select_all(active_peers).await;
        active_peers = others;

        match res {
            Result::Err((address, duration)) => println!(
                "Peer died {}. Active {}.  Keep-alive timer: {}",
                address,
                active_peers.len(),
                duration.time_fmt()
            ),
            Result::Ok(addr) => println!("{}: Success", addr)
        }
    }
}
