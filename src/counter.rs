use std::collections::HashMap;
use std::hash::Hash;

use flume::Receiver;
use flume::Sender;
use futures::StreamExt;

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
