use crate::PieceMeta;
use crate::protocol::BlockRequest;
use sha1::Sha1;
use sha1::Digest;
use std::cmp::min;

const BLOCK_LENGTH: usize = 1 << 14;

pub struct Job {
    piece: PieceMeta,
    blocks: Vec<BlockRequest>,
    data: Vec<u8>,
    in_flight: Vec<BlockRequest>,
}

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
            in_flight: Vec::new()
        }
    }
}