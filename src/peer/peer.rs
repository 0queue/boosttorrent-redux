use std::cmp::min;
use std::time::Duration;

use async_std::net::SocketAddrV4;
use async_std::net::TcpStream;
use async_std::sync::Arc;
use async_std::sync::RwLock;
use bit_vec::BitVec;
use futures::AsyncReadExt;
use sha1::Digest;
use sha1::Sha1;

use crate::count_ones;
use crate::data::DownloadedPiece;
use crate::data::MessageBus;
use crate::data::PeerBus;
use crate::data::SharedState;
use crate::data::Us;
use crate::peer::protocol;
use crate::peer::protocol::BlockRequest;
use crate::peer::protocol::message::Message;
use crate::PieceMeta;

pub struct Peer {
    pub addr: SocketAddrV4,
    peer_bus: PeerBus,
    msg_bus: Option<MessageBus>,
    state: State,
}

struct State {
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
    pub fn new(addr: SocketAddrV4, peer_bus: PeerBus, num_pieces: usize) -> Peer {
        Peer {
            addr,
            peer_bus,
            msg_bus: None,
            state: State {
                choked: true,
                interested: false,
                bitfield: bit_vec::BitVec::from_elem(num_pieces, false),
            },
        }
    }

    pub async fn start(
        mut self,
        us: Us,
        shared_state: Arc<RwLock<SharedState>>,
        endgame_pieces: Vec<PieceMeta>,
    ) -> Result<SocketAddrV4, SocketAddrV4> {
        println!("{}: Starting", self.addr);

        let stream = async_std::io::timeout(Duration::from_secs(5), TcpStream::connect(self.addr));
        let mut stream = match stream.await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}: Failed to start: {}", self.addr, e);
                return Err(self.addr);
            }
        };

        if let Err(e) = protocol::handshake(&mut stream, &us).await {
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

        self.main(shared_state, endgame_pieces).await
    }

    async fn main(
        mut self,
        shared_state: Arc<RwLock<SharedState>>,
        mut endgame_pieces: Vec<PieceMeta>,
    ) -> Result<SocketAddrV4, SocketAddrV4> {
        let mut job: Option<Job> = None;

        self.send(Message::Interested);

        match self.msg_bus.as_mut().unwrap().rx.recv_async().await {
            Ok(Message::Bitfield(bitfield)) => self.state.bitfield = bitfield,
            _ => {
                eprintln!("{}: did not receive bitfield", self.addr);
                return Err(self.addr);
            }
        }

        loop {
            // println!("{}: main", self.addr);

            // get job from queue if needed
            if job.is_none() && count_ones(&self.state.bitfield) > 0 {
                // TODO could also have a counter, that breaks at 20
                //   in order to allow more msg processing
                // while job.is_none() {
                match self.peer_bus.work_queue.pop() {
                    Ok(m) => {
                        if self.state.bitfield[m.index] {
                            println!("{}: took {}", self.addr, m.index);
                            job.replace(create_job(m));
                        } else {
                            self.peer_bus.work_queue.push(m);
                            async_std::task::yield_now().await;
                        }
                    }
                    Err(_) => {
                        // OKAY need endgame mode
                        // eprintln!("{}: error getting work", self.addr);
                        match endgame_pieces.pop() {
                            Some(m) => {
                                if self.state.bitfield[m.index] {
                                    println!("{}: endgame {}", self.addr, m.index);
                                    job.replace(create_job(m));
                                }
                            }
                            None => { /* nothing */ }
                        }
                    }
                };
                // }
            }

            // read lifecycle state, if not dead, try to read for 5 seconds
            if shared_state.read().await.done {
                return Ok(self.addr);
            }

            // handle messages
            let block = job.as_ref().map(|j| j.in_flight > 0).unwrap_or(true) || self.state.choked;
            if !self.handle_msg(&mut job, block).await {
                return Err(self.addr);
            }

            // if job done, send it away, else, else fill pipeline if possible
            let job_state = job.as_ref().map(|j| j.state());
            match job_state {
                Some(JobState::Failed) => {
                    let j = job.unwrap();
                    eprintln!("{}: failed to get {}", self.addr, j.piece.index);
                    self.peer_bus.work_queue.push(j.piece);
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
                    }
                }
                None => {}
            }

            async_std::task::yield_now().await;
        }
    }

    fn send(&self, msg: Message) {
        // println!("{}: ---> {}", self.addr, msg);
        self.msg_bus.as_ref().unwrap().tx.send(msg).unwrap();
    }

    async fn handle_msg(&mut self, job: &mut Option<Job>, block: bool) -> bool {
        let msg = if block {
            match async_std::future::timeout(
                Duration::from_secs(5),
                self.msg_bus.as_mut().unwrap().rx.recv_async(),
            )
            .await
            {
                Ok(Ok(m)) => m,
                Ok(Err(_)) => return false,
                Err(_) => return true,
            }
        } else {
            match self.msg_bus.as_mut().unwrap().rx.try_recv() {
                Ok(m) => m,
                // all good to exit here
                Err(_) => return true,
            }
        };

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
                    None => return true,
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

        true
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
