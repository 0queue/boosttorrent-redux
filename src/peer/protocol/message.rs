use async_std::net::TcpStream;
use byteorder::BigEndian;
use byteorder::ByteOrder;
use futures::io::ReadHalf;
use futures::io::WriteHalf;
use futures::AsyncReadExt;
use futures::AsyncWriteExt;

use crate::peer::protocol::BlockRequest;
use crate::peer::protocol::BlockResponse;

#[derive(Debug)]
pub enum Message {
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have(u32),
    Bitfield(bit_vec::BitVec),
    Request(BlockRequest),
    Piece(BlockResponse),
    Cancel(BlockRequest),
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let res = match self {
            Message::Choke => "Message::Choke".to_string(),
            Message::Unchoke => "Message::Unchoke".to_string(),
            Message::Interested => "Message::Interested".to_string(),
            Message::NotInterested => "Message::NotInterested".to_string(),
            Message::Have(index) => format!("Message::Have({})", index),
            Message::Bitfield(_) => "Message::Bitfield(...)".to_string(),
            Message::Request(req) => format!(
                "Message::Request(index: {}, begin: {}, length: {}",
                req.index, req.begin, req.length
            ),
            Message::Piece(block) => format!(
                "Message::Piece(index: {}, begin: {}, ...)",
                block.index, block.begin
            ),
            Message::Cancel(req) => format!(
                "Message::Cancel(index: {}, begin: {}, length: {}",
                req.index, req.begin, req.length
            ),
        };

        write!(f, "{}", res)
    }
}

impl Message {
    pub async fn from(stream: &mut ReadHalf<TcpStream>) -> Result<Message, &'static str> {
        let length = loop {
            let mut length_prefix = [0u8; 4];
            stream.read_exact(&mut length_prefix).await.map_err(|e| {
                eprintln!("stream read error: {}", e);
                "failed to read prefix"
            })?;
            let length = BigEndian::read_u32(&length_prefix[0..]);
            if length != 0 {
                break length;
            } else {
                println!("received keepalive");
            }
        };

        // println!("Received length of {}", length);

        let mut body = vec![0u8; length as usize];
        stream.read_exact(&mut body).await.map_err(|_| {
            eprintln!("error reading {} bytes", length);
            "failed to read body"
        })?;

        let id = body[0];
        let body = &body[1..];

        let msg = match id {
            0 => Message::Choke,
            1 => Message::Unchoke,
            2 => Message::Interested,
            3 => Message::NotInterested,
            4 => Message::Have(BigEndian::read_u32(&body)),
            5 => Message::Bitfield(bit_vec::BitVec::from_bytes(&body)),
            6 => Message::Request(BlockRequest {
                index: BigEndian::read_u32(&body[0..4]),
                begin: BigEndian::read_u32(&body[4..8]),
                length: BigEndian::read_u32(&body[8..12]),
            }),
            7 => Message::Piece(BlockResponse {
                index: BigEndian::read_u32(&body[0..4]),
                begin: BigEndian::read_u32(&body[4..8]),
                data: Vec::from(&body[8..]),
            }),
            8 => Message::Cancel(BlockRequest {
                index: BigEndian::read_u32(&body[0..4]),
                begin: BigEndian::read_u32(&body[4..8]),
                length: BigEndian::read_u32(&body[8..12]),
            }),
            _ => return Err("Unrecognized msg id"),
        };

        Ok(msg)
    }

    pub async fn send(&self, stream: &mut WriteHalf<TcpStream>) -> Result<(), ()> {
        let buf = match self {
            Message::Choke => prepare_buf(0, 0),
            Message::Unchoke => prepare_buf(0, 1),
            Message::Interested => prepare_buf(0, 2),
            Message::NotInterested => prepare_buf(0, 3),
            Message::Have(idx) => {
                let mut buf = prepare_buf(4, 4);
                BigEndian::write_u32(&mut buf[5..], *idx);
                buf
            }
            Message::Bitfield(v) => {
                let payload = v.to_bytes();
                let mut buf = prepare_buf(payload.len() as u32, 5);
                buf[5..].copy_from_slice(&payload);
                buf
            }
            Message::Request(block_req) => {
                let mut buf = prepare_buf(4 * 3, 6);
                BigEndian::write_u32(&mut buf[5..], block_req.index);
                BigEndian::write_u32(&mut buf[9..], block_req.begin);
                BigEndian::write_u32(&mut buf[13..], block_req.length);
                buf
            }
            Message::Piece(block_resp) => {
                let mut buf = prepare_buf((4 + 4 + block_resp.data.len()) as u32, 7);
                BigEndian::write_u32(&mut buf[5..], block_resp.index);
                BigEndian::write_u32(&mut buf[9..], block_resp.begin);
                buf[13..].copy_from_slice(&block_resp.data);
                buf
            }
            Message::Cancel(block_req) => {
                let mut buf = prepare_buf(4 * 3, 8);
                BigEndian::write_u32(&mut buf[5..], block_req.index);
                BigEndian::write_u32(&mut buf[9..], block_req.begin);
                BigEndian::write_u32(&mut buf[13..], block_req.length);
                buf
            }
        };

        stream.write_all(&buf).await.map_err(|_| ())
    }
}

fn prepare_buf(payload_length: u32, msg_type: u8) -> Vec<u8> {
    let mut res = vec![0u8; 4 + 1 + payload_length as usize];
    BigEndian::write_u32(&mut res[0..4], payload_length + 1);
    res[4] = msg_type;

    res
}
