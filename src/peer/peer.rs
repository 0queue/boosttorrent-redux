use std::cmp::min;
use std::time::Duration;

use async_std::future::timeout;
use async_std::future::TimeoutError;
use async_std::net::SocketAddrV4;
use async_std::net::TcpStream;
use bit_vec::BitVec;
use futures::AsyncReadExt;
use futures::Future;
use sha1::Digest;
use sha1::Sha1;
use util::ext::bitvec::BitVecExt;

use crate::counter::Event;
use crate::data::DownloadedPiece;
use crate::data::Lifecycle;
use crate::data::MessageBus;
use crate::data::PeerBus;
use crate::data::SharedState;
use crate::protocol;
use crate::protocol::BlockRequest;
use crate::protocol::message::Message;
use crate::PieceMeta;
use util::timer::Timer;

const BLOCK_LENGTH: usize = 1 << 14;
const PIPELINE_LENGTH: usize = 15;

pub struct Peer {
    pub addr: SocketAddrV4,
    peer_bus: PeerBus,
    state: PeerState,
    shared_state: SharedState,
}

struct PeerState {
    choked: bool,
    interested: bool,
    bitfield: BitVec,
    haves_idx: usize,
}

struct Job {
    piece: PieceMeta,
    blocks: Vec<BlockRequest>,
    data: Vec<u8>,
    in_flight: usize,
}

enum JobState {
    InProgress,
    Failed,
    Success,
}

impl Peer {
    pub fn new(
        addr: SocketAddrV4,
        peer_bus: PeerBus,
        num_pieces: usize,
        shared_state: SharedState,
    ) -> Peer {
        Peer {
            addr,
            peer_bus,
            state: PeerState {
                choked: true,
                interested: false,
                bitfield: bit_vec::BitVec::from_elem(num_pieces, false),
                haves_idx: 0,
            },
            shared_state,
        }
    }

    pub async fn start(self) -> Result<SocketAddrV4, (SocketAddrV4, Duration)> {
        println!("{}: Starting", self.addr);

        let stream = async_std::io::timeout(Duration::from_secs(45), TcpStream::connect(self.addr));
        let mut stream = match stream.await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}: Failed to start: {}", self.addr, e);
                return Err((self.addr, Duration::from_millis(0)));
            }
        };

        let (id, file_hash) = {
            let s = self.shared_state.read().await;
            (s.id, s.file_hash)
        };
        if let Err(e) = protocol::handshake(&mut stream, &id, &file_hash).await {
            eprintln!("{}: Failed to handshake: {}", self.addr, e);
        }

        let (stream_rx, stream_tx) = stream.split();
        let (send_msg_tx, send_msg_rx) = flume::unbounded();
        let (recv_msg_tx, recv_msg_rx) = flume::unbounded();

        let msg_bus = MessageBus::new(send_msg_tx, recv_msg_rx);

        async_std::task::spawn(protocol::sender(stream_tx, send_msg_rx));
        async_std::task::spawn(protocol::receiver(stream_rx, recv_msg_tx));

        println!("{}: Successful start", self.addr);

        self.main(msg_bus).await
    }

    async fn main(mut self, mut msg_bus: MessageBus) -> Result<SocketAddrV4, (SocketAddrV4, Duration)> {
        let mut job: Option<Job> = None;
        let mut timer = Timer::new();
        let mut keepalive = Timer::new();
        timer.start(); // because we start out choked

        msg_bus.send(Message::Interested);

        match msg_bus.recv().await {
            Ok(Message::Bitfield(bitfield)) => self.state.bitfield = bitfield,
            _ => {
                // TODO not necessary to exit here
                eprintln!("{}: did not receive bitfield", self.addr);
                return Err((self.addr, Duration::from_millis(0)));
            }
        }

        // The phases are:
        // - get job (if none selected, there are pieces to get, and not choked
        // - if done, exit
        // - read message.  if requests are in flight, or choked, or there is no job, block
        // - handle message by updating state as needed
        // - if job is done, report, else fill pipeline

        loop {
            // get job from queue if needed
            if self.get_job(&mut job).await {
                return Ok(self.addr);
            };

            // read lifecycle state, if not dead, try to read for 5 seconds
            if self.shared_state.read().await.lifecycle == Lifecycle::Done {
                return Ok(self.addr);
            }

            // handle messages
            let block = job.as_ref().map(|j| j.in_flight > 0).unwrap_or(true) || self.state.choked;
            let msg = if block {
                match t(msg_bus.recv()).await {
                    Ok(Ok(m)) => Some(m),
                    Ok(Err(_)) => {
                        if let Some(j) = job {
                            self.peer_bus.work_tx.send(j.piece.index).await;
                        }
                        return Err((self.addr, keepalive.time().unwrap_or(Duration::from_millis(0))));
                    }
                    Err(TimeoutError { .. }) => None,
                }
            } else {
                msg_bus.try_recv().ok()
            };

            if let Some(m) = msg {
                self.handle_msg(&mut job, &mut timer, m)
            }

            for have in self.get_haves().await {
                msg_bus.send(Message::Have(have as u32));
            }

            keepalive.start();

            // TODO process haves and cancel as necessary here

            // if job done, send it away, else, else fill pipeline if possible
            let job_state = job.as_ref().map(|j| j.state());
            match job_state {
                Some(JobState::Failed) => {
                    let j = job.unwrap();
                    eprintln!("{}: failed to get {}", self.addr, j.piece.index);
                    self.peer_bus.work_tx.send(j.piece.index).await;
                    job = None;
                }
                Some(JobState::Success) => {
                    let j = job.unwrap();
                    // msg_bus.send(Message::Have(j.piece.index as u32));
                    // keepalive.start();
                    self.peer_bus.counter_tx.send(Event::RecvPiece(self.addr)).unwrap();
                    self.peer_bus
                        .done_tx
                        .send(DownloadedPiece {
                            index: j.piece.index,
                            hash: j.piece.hash,
                            data: j.data,
                        })
                        .unwrap();
                    job = None;
                }
                Some(JobState::InProgress) => if !self.state.choked {
                    let mut j = job.unwrap();
                    // fill pipeline
                    while j.in_flight < PIPELINE_LENGTH && j.blocks.len() > 0 {
                        msg_bus.send(Message::Request(j.blocks.pop().unwrap()));
                        keepalive.start();
                        j.in_flight += 1;
                    }
                    job = Some(j)
                } else if timer.time().unwrap() > Duration::from_secs(60) {
                    let j = job.unwrap();
                    eprintln!("{}: Choked for over a minute, putting {} back", self.addr, j.piece.index);
                    self.peer_bus.work_tx.send(j.piece.index).await;
                    job = None;
                },
                None => {}
            }

            if let Some(d) = keepalive.time() {
                if d > Duration::from_secs(90) {
                    msg_bus.send(Message::KeepAlive);
                    keepalive.start();
                }
            }

            async_std::task::yield_now().await;
        }
    }

    async fn get_job(&mut self, job: &mut Option<Job>) -> bool {
        if job.is_none() && self.state.bitfield.ones().len() > 0 && !self.state.choked {
            match t(self.peer_bus.work_rx.recv()).await {
                Ok(Some(m)) => {
                    if self.state.bitfield[m] {
                        job.replace(create_job(self.peer_bus.pieces[m].clone()));
                    } else {
                        self.peer_bus.work_tx.send(m).await;
                    }
                }
                Err(_) | Ok(None) => {
                    match t(self.peer_bus.endgame_rx.recv()).await {
                        Ok(Ok(m)) => if self.state.bitfield[m] {
                            job.replace(create_job(self.peer_bus.pieces[m].clone()));
                        },
                        Ok(Err(_)) => {
                            // None in queue, time to wrap up
                            return true;
                        }
                        Err(_) => {
                            // not sure if timeout is necessary here
                        }
                    }
                }
            };
        }

        return false;
    }

    fn handle_msg(&mut self, job: &mut Option<Job>, timer: &mut Timer, msg: Message) {
        match msg {
            Message::Choke => {
                timer.start();
                self.state.choked = true;
            }
            Message::Unchoke => {
                timer.stop();
                self.state.choked = false;
            }
            Message::Interested => self.state.interested = true,
            Message::NotInterested => self.state.interested = false,
            Message::Have(index) => self.state.bitfield.set(index as usize, true),
            Message::Bitfield(bitfield) => self.state.bitfield = bitfield,
            Message::Request(_) => {
                // TODO
                self.peer_bus.counter_tx.send(Event::ReqPiece(self.addr)).unwrap();
            }
            Message::Piece(response) => {
                let mut job = match job {
                    Some(j) => j,
                    None => return,
                };

                if response.index == job.piece.index as u32 {
                    let start = response.begin as usize;
                    let end = start + response.data.len();
                    job.data[start..end].copy_from_slice(&response.data);
                    job.in_flight -= 1;
                }
            }
            Message::Cancel(_) => { /*TODO*/ }
            Message::KeepAlive => { /*nothing*/ }
        }
    }

    async fn get_haves(&mut self) -> Vec<usize> {
        let read_guard = self.peer_bus.haves.read().await;
        let end = read_guard.len();
        let haves = if self.state.haves_idx < end {
            read_guard[self.state.haves_idx..end].to_vec()
        } else {
            vec![]
        };

        self.state.haves_idx = end;
        // if haves.len() > 0 {
        //     println!("{}: sending {} haves", self.addr, haves.len());
        // }

        haves
    }
}

impl Job {
    fn state(&self) -> JobState {
        if self.in_flight > 0 || self.blocks.len() > 0 {
            return JobState::InProgress;
        }

        if Sha1::digest(&self.data).as_slice() == self.piece.hash {
            JobState::Success
        } else {
            JobState::Failed
        }
    }
}

fn create_job(meta: PieceMeta) -> Job {
    let blocks: Vec<BlockRequest> = {
        let mut begin = 0;
        let mut blocks = Vec::new();

        while begin < meta.length {
            blocks.push(BlockRequest {
                index: meta.index as u32,
                begin: begin as u32,
                length: min((meta.length - begin) as u32, BLOCK_LENGTH as u32),
            });

            begin += BLOCK_LENGTH;
        }

        blocks
    };

    let length = meta.length;
    Job {
        piece: meta,
        blocks,
        data: vec![0u8; length],
        in_flight: 0,
    }
}

async fn t<F, T>(fut: F) -> Result<T, TimeoutError>
    where
        F: Future<Output=T>,
{
    timeout(Duration::from_secs(5), fut).await
}
