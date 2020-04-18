use std::sync::Arc;
use std::sync::RwLock;

use flume::RecvError;
use flume::TryRecvError;

#[derive(Clone)]
pub struct Sender<T: Clone> {
    senders: Arc<RwLock<Vec<flume::Sender<T>>>>,
}

impl<T> Sender<T>
where
    T: Clone,
{
    pub fn send(&self, t: T) {
        self.senders
            .write()
            .unwrap()
            .retain(|tx| tx.send(t.clone()).is_ok());
    }
}

pub struct Receiver<T> {
    senders: Arc<RwLock<Vec<flume::Sender<T>>>>,
    receiver: flume::Receiver<T>,
}

impl<T> Receiver<T>
where
    T: Clone,
{
    pub async fn recv(&mut self) -> Result<T, RecvError> {
        self.receiver.recv_async().await
    }

    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.receiver.try_recv()
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        let (tx, rx) = flume::unbounded();
        self.senders.write().unwrap().push(tx);
        Receiver {
            senders: self.senders.clone(),
            receiver: rx,
        }
    }

    fn clone_from(&mut self, _source: &Self) {
        unimplemented!()
    }
}

pub fn unbounded<T: Clone>() -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = flume::unbounded();
    let shared = Arc::new(RwLock::new(vec![tx]));
    (
        Sender {
            senders: shared.clone(),
        },
        Receiver {
            senders: shared.clone(),
            receiver: rx,
        },
    )
}
