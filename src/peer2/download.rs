use std::time::Duration;

use async_std::future;
use async_std::future::TimeoutError;
use async_std::net::SocketAddrV4;
use async_std::net::TcpStream;
use futures::AsyncReadExt;
use rand::seq::SliceRandom;
use util::ext::bitvec::BitVecExt;
use util::ext::duration::SecondsExt;

use crate::controller::{DownloadedPiece, Lifecycle};
use crate::counter::Event;
use crate::counter::Event::ReqPiece;
use crate::peer2::job::JobState;
use crate::peer2::Peer2;
use crate::protocol;
use crate::protocol::BlockRequest;
use crate::protocol::message::bus::MessageBus;
use crate::protocol::message::Message;

const PIPELINE_LENGTH: usize = 15;

impl Peer2 {
    pub async fn start(mut self) -> Result<SocketAddrV4, (SocketAddrV4, Duration)> {
        println!("{}: starting", self.addr);

        let stream = async_std::io::timeout(30.secs(), TcpStream::connect(self.addr));
        let mut stream = stream.await.map_err(|e| {
            eprintln!("{}: Failed to start {}", self.addr, e);
            (self.addr, 0.secs())
        })?;

        let (id, file_hash) = (self.torrent_info.id, self.torrent_info.file_hash);
        protocol::handshake(&mut stream, &id, &file_hash).await.map_err(|e| {
            eprintln!("{}: Failed to handshake {}", self.addr, e);
            (self.addr, 0.secs())
        })?;

        let (stream_rx, stream_tx) = stream.split();
        let (send_msg_tx, send_msg_rx) = flume::unbounded();
        let (recv_msg_tx, recv_msg_rx) = flume::unbounded();
        let mut msg_bus = MessageBus::new(send_msg_tx, recv_msg_rx);

        async_std::task::spawn(protocol::sender(stream_tx, send_msg_rx));
        async_std::task::spawn(protocol::receiver(stream_rx, recv_msg_tx));

        msg_bus.send(Message::Interested);

        // assume they will send a bitfield.  not strictly true though
        match msg_bus.recv().await {
            Ok(Message::Bitfield(bitfield)) => self.peer_state.bitfield = bitfield,
            _ => {
                eprintln!("{}: failed to receive bitfield", self.addr);
                return Err((self.addr, 0.secs()));
            }
        }

        println!("{}: successful start", self.addr);

        self.peer_state.choke_timer.start();

        self.main(msg_bus).await
    }

    async fn main(mut self, mut msg_bus: MessageBus) -> Result<SocketAddrV4, (SocketAddrV4, Duration)> {
        loop {
            if self.controller_state.read().await.lifecycle == Lifecycle::Done {
                return Ok(self.addr);
            }

            // get job
            self.get_job().await;

            // read msgs for up to 5 seconds
            self.handle_msgs(&mut msg_bus).await?;

            // send haves and cancels
            for have in self.get_haves().await {
                msg_bus.send(Message::Have(have as u32));
                for cancel in self.cancel(have) {
                    msg_bus.send(Message::Cancel(cancel));
                }
            }

            // process current job state
            self.check_job(&mut msg_bus).await;

            // if choked for too long, give up on this piece
            if self.peer_state.choke_timer.time().map(|d| d > 60.secs()).unwrap_or(false) {
                if let Some(j) = self.job.take() {
                    eprintln!("{}: Choked for over a minute, putting {} back", self.addr, j.piece.index);
                    self.controller_bus.work_bus.tx.send(j.piece.index).await;
                }
            }

            // check keep alive timer
            if msg_bus.last_sent().unwrap() > 90.secs() {
                msg_bus.send(Message::KeepAlive);
            }
        };
    }

    /// try once to get a job from either the work queue or in endgame mode
    async fn get_job(&mut self) {
        if self.job.is_some() || self.peer_state.bitfield.ones().len() == 0 || self.peer_state.choked {
            return;
        }

        let lifecycle = self.controller_state.read().await.lifecycle;

        // TODO add a counter to try x times before giving up?
        match lifecycle {
            Lifecycle::Downloading => {
                let fut = future::timeout(5.secs(), self.controller_bus.work_bus.rx.recv());
                if let Ok(Some(i)) = fut.await {
                    if self.peer_state.bitfield[i] {
                        self.job.replace(self.torrent_info.pieces[i].clone().into());
                    } else {
                        self.controller_bus.work_bus.tx.send(i).await;
                    }
                }
            }
            Lifecycle::Endgame => {
                let zeroes = self.controller_state.read().await.bitfield.zeroes();
                if let Some(&i) = zeroes.choose(&mut rand::thread_rng()) {
                    if self.peer_state.bitfield[i] {
                        self.job.replace(self.torrent_info.pieces[i].clone().into());
                    }
                }
            }
            Lifecycle::Done => {}
        };

        // println!("{}: job {:?}", self.addr, self.job);
    }

    async fn handle_msgs(&mut self, msg_bus: &mut MessageBus) -> Result<(), (SocketAddrV4, Duration)> {
        // Block if:
        //  - there are requests we are waiting on the response for
        //  - there is no job? not sure about that one TODO
        //  - we are choked (wait for unchoke)
        let block = self.job.as_ref().map(|j| j.in_flight.len() > 0).unwrap_or(true) || self.peer_state.choked;
        let msg = if block {
            match future::timeout(5.secs(), msg_bus.recv()).await {
                Ok(Ok(msg)) => Some(msg),
                Ok(Err(_)) => {
                    if let Some(ref j) = self.job {
                        self.controller_bus.work_bus.tx.send(j.piece.index).await;
                    }

                    return Err((self.addr, msg_bus.last_sent().unwrap_or(0.secs())));
                }
                Err(x) => None
            }
        } else {
            msg_bus.try_recv().ok()
        };

        // println!("{}: handling msg {:?} (blocked {})", self.addr, msg, block);

        if let Some(msg) = msg {
            self.update(msg).await;
        }

        Ok(())
    }

    async fn update(&mut self, msg: Message) {
        match msg {
            Message::Choke => {
                self.peer_state.choked = true;
                self.peer_state.choke_timer.start();
            }
            Message::Unchoke => {
                self.peer_state.choked = false;
                self.peer_state.choke_timer.stop();
            }
            Message::Interested => self.peer_state.interested = true,
            Message::NotInterested => self.peer_state.interested = false,
            Message::Have(i) => self.peer_state.bitfield.set(i as usize, true),
            Message::Bitfield(bitfield) => self.peer_state.bitfield = bitfield,
            Message::Request(req) => {
                // TODO
                self.controller_bus.counter_tx.send(ReqPiece(self.addr)).unwrap();
            }
            Message::Piece(response) => if let Some(ref mut j) = self.job {
                j.response(&response);
            },
            Message::Cancel(_) => { /* TODO */ }
            Message::KeepAlive => { /* nothing */ }
        }
    }

    async fn get_haves(&mut self) -> Vec<usize> {
        let read_guard = self.controller_state.read().await;
        let end = read_guard.haves.len();
        let haves = if self.haves_idx < end {
            read_guard.haves[self.haves_idx..end].to_vec()
        } else {
            vec![]
        };

        self.haves_idx = end;

        haves
    }

    fn cancel(&mut self, have: usize) -> Vec<BlockRequest> {
        let cur = self.job.as_ref().map(|j| j.piece.index);

        if cur == Some(have) {
            self.job.take().unwrap().cancel()
        } else {
            vec![]
        }
    }

    async fn check_job(&mut self, msg_bus: &mut MessageBus) {
        let job_state = self.job.as_ref().map(|j| j.state());
        // println!("{}: job state {:?}", self.addr, job_state);
        match job_state {
            Some(JobState::Failed) => {
                let j = self.job.take().unwrap();
                eprintln!("{}: failed to get {}", self.addr, j.piece.index);
                self.controller_bus.work_bus.tx.send(j.piece.index).await;
            }
            Some(JobState::Success) => {
                let j = self.job.take().unwrap();
                self.controller_bus.counter_tx.send(Event::RecvPiece(self.addr)).unwrap();
                self.controller_bus
                    .done_tx
                    .send(j.into())
                    .unwrap();
            }
            Some(JobState::InProgress) => if !self.peer_state.choked {
                // fill pipeline
                let mut j = self.job.take().unwrap();

                for req in j.fill_to(PIPELINE_LENGTH) {
                    msg_bus.send(Message::Request(req));
                }

                self.job.replace(j);
            }
            None => {}
        }
    }
}