use std::cmp::min;
use std::time::Duration;

use async_std::future::timeout;
use async_std::net::SocketAddrV4;
use async_std::net::TcpStream;
use bit_vec::BitVec;
use flume::RecvError;
use flume::TryRecvError;
use futures::AsyncReadExt;
use sha1::Digest;
use sha1::Sha1;

use crate::count_ones;
use crate::data::MessageBus;
use crate::data::PeerBus;
use crate::data::SharedState;
use crate::data::{DownloadedPiece, Lifecycle};
use crate::peer::protocol;
use crate::peer::protocol::message::Message;
use crate::peer::protocol::BlockRequest;
use crate::PieceMeta;

pub struct Peer {
    pub addr: SocketAddrV4,
    peer_bus: PeerBus,
    msg_bus: Option<MessageBus>,
    state: PeerState,
    shared_state: SharedState,
}

struct PeerState {
    choked: bool,
    interested: bool,
    bitfield: BitVec,
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
            msg_bus: None,
            state: PeerState {
                choked: true,
                interested: false,
                bitfield: bit_vec::BitVec::from_elem(num_pieces, false),
            },
            shared_state,
        }
    }

    pub async fn start(mut self) -> Result<SocketAddrV4, SocketAddrV4> {
        println!("{}: Starting", self.addr);

        let stream = async_std::io::timeout(Duration::from_secs(5), TcpStream::connect(self.addr));
        let mut stream = match stream.await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}: Failed to start: {}", self.addr, e);
                return Err(self.addr);
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

        let msg_bus = MessageBus {
            tx: send_msg_tx,
            rx: recv_msg_rx,
        };

        async_std::task::spawn(protocol::sender(stream_tx, send_msg_rx));
        async_std::task::spawn(protocol::receiver(stream_rx, recv_msg_tx));

        // can unwrap from here on
        self.msg_bus.replace(msg_bus);

        println!("{}: Successful start", self.addr);

        self.main().await
    }

    async fn main(mut self) -> Result<SocketAddrV4, SocketAddrV4> {
        let mut job: Option<Job> = None;

        self.send(Message::Interested);

        match self.recv().await {
            Ok(Message::Bitfield(bitfield)) => self.state.bitfield = bitfield,
            _ => {
                eprintln!("{}: did not receive bitfield", self.addr);
                return Err(self.addr);
            }
        }

        loop {
            // get job from queue if needed
            if job.is_none() && count_ones(&self.state.bitfield) > 0 && !self.state.choked {
                match timeout(Duration::from_secs(2), self.peer_bus.work_rx.recv()).await {
                    Ok(Some(m)) => {
                        if self.state.bitfield[m.index] {
                            job.replace(create_job(m));
                        } else {
                            self.peer_bus.work_tx.send(m).await;
                        }
                    }
                    Err(_) | Ok(None) => {
                        match timeout(Duration::from_secs(5), self.peer_bus.endgame_rx.recv()).await
                        {
                            Ok(Ok(m)) => {
                                job.replace(create_job(m));
                            }
                            Ok(Err(_)) => {
                                return Ok(self.addr);
                            }
                            Err(_) => { /*nothing*/ }
                        }
                    }
                };
            }

            // read lifecycle state, if not dead, try to read for 5 seconds
            if self.shared_state.read().await.lifecycle == Lifecycle::Done {
                return Ok(self.addr);
            }

            // handle messages
            let block = job.as_ref().map(|j| j.in_flight > 0).unwrap_or(true) || self.state.choked;
            let msg = if block {
                match timeout(Duration::from_secs(5), self.recv()).await {
                    Ok(Ok(m)) => Some(m),
                    Err(_) => None,
                    Ok(Err(_)) => {
                        if let Some(j) = job {
                            self.peer_bus.work_tx.send(j.piece).await;
                        }
                        return Err(self.addr);
                    }
                }
            } else {
                self.try_recv().ok()
            };

            if let Some(m) = msg {
                self.handle_msg(&mut job, m)
            }

            // if job done, send it away, else, else fill pipeline if possible
            let job_state = job.as_ref().map(|j| j.state());
            match job_state {
                Some(JobState::Failed) => {
                    let j = job.unwrap();
                    eprintln!("{}: failed to get {}", self.addr, j.piece.index);
                    self.peer_bus.work_tx.send(j.piece).await;
                    job = None;
                }
                Some(JobState::Success) => {
                    let j = job.unwrap();
                    self.send(Message::Have(j.piece.index as u32));
                    self.peer_bus.counter_tx.send(self.addr).unwrap();
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
                Some(JobState::InProgress) => {
                    if !self.state.choked {
                        let mut j = job.unwrap();
                        // fill pipeline
                        while j.in_flight < PIPELINE_LENGTH && j.blocks.len() > 0 {
                            self.send(Message::Request(j.blocks.pop().unwrap()));
                            j.in_flight += 1;
                        }
                        job = Some(j)
                    } else {
                        // TODO check how long we've been choked
                        //   if too long, put work back (1 min?)
                    }
                }
                None => {}
            }

            async_std::task::yield_now().await;
        }
    }

    fn handle_msg(&mut self, job: &mut Option<Job>, msg: Message) {
        // println!("{}: received {}", self.addr, msg);
        match msg {
            Message::Choke => self.state.choked = true,
            Message::Unchoke => self.state.choked = false,
            Message::Interested => self.state.interested = true,
            Message::NotInterested => self.state.interested = false,
            Message::Have(index) => self.state.bitfield.set(index as usize, true),
            Message::Bitfield(bitfield) => self.state.bitfield = bitfield,
            Message::Request(_) => { /*TODO*/ }
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
        }
    }

    fn send(&self, msg: Message) {
        self.msg_bus.as_ref().unwrap().tx.send(msg).unwrap();
    }

    async fn recv(&mut self) -> Result<Message, RecvError> {
        self.msg_bus.as_mut().unwrap().rx.recv_async().await
    }

    fn try_recv(&mut self) -> Result<Message, TryRecvError> {
        self.msg_bus.as_mut().unwrap().rx.try_recv()
    }
}

impl Job {
    fn state(&self) -> JobState {
        if self.in_flight > 0 || self.blocks.len() > 0 {
            return JobState::InProgress;
        }

        let mut hasher = Sha1::new();
        hasher.input(&self.data);
        let hash = hasher.result();

        if hash.as_slice() == self.piece.hash {
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

const BLOCK_LENGTH: usize = 1 << 14;
const PIPELINE_LENGTH: usize = 10;
