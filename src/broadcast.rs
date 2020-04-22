use std::sync::Arc;
use std::sync::RwLock;

use flume::RecvError;
use rand::seq::SliceRandom;

#[derive(Clone)]
pub struct Sender<T: Clone> {
    senders: Arc<RwLock<Vec<flume::Sender<T>>>>,
}

impl<T> Sender<T>
where
    T: Clone,
{
    #[allow(dead_code)]
    pub fn send(&self, t: T) {
        self.senders
            .write()
            .unwrap()
            .retain(|tx| tx.send(t.clone()).is_ok());
    }

    pub fn random_send_all(&self, ts: &[T]) {
        let mut ts = ts.to_vec();

        let mut rand = rand::thread_rng();

        self.senders
            .write()
            .unwrap()
            .retain(|tx| {
                ts.shuffle(&mut rand);
                for t in &ts {
                    if !tx.send(t.clone()).is_ok() {
                        return false
                    }
                }

                true
            });
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
