use async_std::net::SocketAddrV4;
use async_std::sync::Arc;
use bit_vec::BitVec;
use util::ext::duration::DurationExt;
use util::timer::Timer;

use crate::controller::ControllerBus;
use crate::controller::ControllerState;
use crate::controller::TorrentInfo;
use crate::peer::job::Job;
use std::sync::atomic::AtomicBool;

mod job;
mod download;

/// Our state, meta info (addr, torrent),
/// channels (in busses) and shared state
pub struct Peer {
    pub addr: SocketAddrV4,
    torrent_info: Arc<TorrentInfo>,
    running_flag: Arc<AtomicBool>,
    controller_bus: ControllerBus,
    peer_state: PeerState,
    controller_state: ControllerState,
    haves_idx: usize,
    job: Option<Job>,
}

/// State related to our peer
/// - are we choked?
/// - are they interested?
/// - what do they have?
struct PeerState {
    choked: bool,
    interested: bool,
    bitfield: BitVec,
    choke_timer: Timer,
}

impl Peer {
    pub async fn new(
        addr: SocketAddrV4,
        torrent_info: Arc<TorrentInfo>,
        running_flag: Arc<AtomicBool>,
        controller_bus: ControllerBus,
        controller_state: ControllerState
    ) -> Self {
        Peer {
            addr,
            torrent_info: torrent_info.clone(),
            running_flag,
            controller_bus,
            peer_state: PeerState {
                choked: true,
                interested: false,
                bitfield: BitVec::from_elem(torrent_info.pieces.len(), false),
                choke_timer: Timer::new(),
            },
            controller_state,
            haves_idx: 0,
            job: None,
        }
    }
}

/// Spawn all the addresses as peers,
/// watch for their death, and announce
pub async fn spawner(
    addresses: Vec<SocketAddrV4>,
    torrent_info: Arc<TorrentInfo>,
    running_flag: Arc<AtomicBool>,
    controller_bus: ControllerBus,
    controller_state: ControllerState,
) {
    let mut active_peers = Vec::new();

    for address in addresses {
        let peer = Peer::new(
            address,
            torrent_info.clone(),
            running_flag.clone(),
            controller_bus.clone(),
            controller_state.clone(),
        ).await;

        active_peers.push(async_std::task::spawn(peer.start()))
    }

    while active_peers.is_empty() {
        let (res, _, remaining) = futures::future::select_all(active_peers).await;
        active_peers = remaining;

        match res {
            Ok(addr) => println!("{}: Success", addr),
            Err((addr, duration)) => println!(
                "{}: Died (keep alive: {}).  Active {}.",
                addr, duration.time_fmt(), active_peers.len()
            )
        }
    }
}