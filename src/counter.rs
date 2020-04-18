use std::collections::HashMap;
use std::hash::Hash;

use flume::Receiver;
use flume::Sender;
use futures::StreamExt;
use async_std::net::SocketAddrV4;

pub struct Counter<T: Hash + Eq> {
    rx: Receiver<T>,
}

impl<T: Hash + Eq> Counter<T> {
    pub fn new() -> (Counter<T>, Sender<T>) {
        let (tx, rx) = flume::unbounded();
        (Counter { rx }, tx)
    }

    pub async fn start(self) -> HashMap<T, u32> {
        let mut res = HashMap::new();

        self.rx
            .for_each(|t| {
                *res.entry(t).or_default() += 1;
                futures::future::ready(())
            })
            .await;

        res
    }
}

#[derive(Hash, Eq, PartialEq, Debug)]
pub enum Event {
    RecvPiece(SocketAddrV4),
    ReqPiece(SocketAddrV4),
}
