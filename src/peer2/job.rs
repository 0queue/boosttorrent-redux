use crate::PieceMeta;
use crate::protocol::{BlockRequest, BlockResponse};
use sha1::Sha1;
use sha1::Digest;
use std::cmp::min;
use crate::controller::DownloadedPiece;
use std::fmt::{Debug, Formatter};

const BLOCK_LENGTH: usize = 1 << 14;

pub struct Job {
    pub piece: PieceMeta,
    blocks: Vec<BlockRequest>,
    pub data: Vec<u8>,
    pub in_flight: Vec<BlockRequest>,
}

#[derive(Debug)]
pub enum JobState {
    InProgress,
    Failed,
    Success,
}

impl Job {
    pub fn state(&self) -> JobState {
        if self.in_flight.len() > 0 || self.blocks.len() > 0 {
            return JobState::InProgress;
        }

        if Sha1::digest(&self.data).as_slice() == self.piece.hash {
            JobState::Success
        } else {
            JobState::Failed
        }
    }

    pub fn response(&mut self, response: &BlockResponse) {
        if response.index != self.piece.index as u32 {
            return;
        }

        let start = response.begin as usize;
        let end = start + response.data.len();
        self.data[start..end].copy_from_slice(&response.data);
        self.in_flight.retain(|e| e.begin != response.begin);
    }

    pub fn cancel(&mut self) -> Vec<BlockRequest> {
        self.in_flight.drain(0..).collect()
    }

    pub fn fill_to(&mut self, n: usize) -> Vec<BlockRequest> {
        let mut res = vec![];
        while self.blocks.len() > 0 && self.in_flight.len() <= n {
            res.push(self.blocks.pop().unwrap());
        }

        self.in_flight.append(&mut res.clone());
        res
    }
}

impl From<PieceMeta> for Job {
    fn from(meta: PieceMeta) -> Self {
        let blocks = {
            let mut begin = 0;
            let mut blocks = Vec::new();

            while begin < meta.length {
                blocks.push(BlockRequest {
                    index: meta.index as u32,
                    begin: begin as u32,
                    length: min(meta.length - begin, BLOCK_LENGTH) as u32,
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
            in_flight: Vec::new(),
        }
    }
}

impl Debug for Job {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Job {{ index: {}, in_flight: {} }}", self.piece.index, self.in_flight.len())
    }
}

impl From<Job> for DownloadedPiece {
    fn from(job: Job) -> Self {
        DownloadedPiece {
            index: job.piece.index,
            hash: job.piece.hash,
            data: job.data,
        }
    }
}