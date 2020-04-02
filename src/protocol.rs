use async_std::io::prelude::WriteExt;
use async_std::net::TcpStream;
use byteorder::BigEndian;
use byteorder::ByteOrder;
use futures::AsyncReadExt;

pub const PROTOCOL: &[u8; 20] = b"\x19Bittorrent protocol";

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

#[derive(Debug)]
pub struct BlockRequest {
    index: u32,
    begin: u32,
    length: u32,
}

impl From<(u32, u32, u32)> for BlockRequest {
    fn from((index, begin, length): (u32, u32, u32)) -> Self {
        BlockRequest {
            index,
            begin,
            length,
        }
    }
}

pub struct BlockResponse {
    index: u32,
    begin: u32,
    data: Vec<u8>,
}

impl Message {
    pub async fn from(stream: &mut TcpStream) -> Result<Message, ()> {
        let mut prefix = [0u8; 5];
        stream.read_exact(&mut prefix).await.map_err(|_| ())?;

        let length = BigEndian::read_u32(&prefix[1..]);

        let mut body= vec![0u8; length as usize];
        stream.read_exact(&mut body).await.map_err(|_| ())?;

        let msg = match prefix[0] {
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
            7 => Message::Piece(BlockResponse{
                index: BigEndian::read_u32(&body[0..4]),
                begin: BigEndian::read_u32(&body[4..8]),
                data: Vec::from(&body[8..])
            }),
            8 => Message::Cancel(BlockRequest {
                index: BigEndian::read_u32(&body[0..4]),
                begin: BigEndian::read_u32(&body[4..8]),
                length: BigEndian::read_u32(&body[8..12]),
            }),
            _ => return Err(()),
        };

        Ok(msg)
    }

    pub async fn send(&self, stream: &mut TcpStream) -> Result<(), ()> {
        let mut length = [0u8; 4];

        match self {
            Message::Choke => {
                stream.write_all(&[0u8; 1]).await.map_err(|_| ())?;
                BigEndian::write_u32(&length, 0);
                stream.write_all(&length).await.map_err(|_| ())?;
            },
            Message::Unchoke => {},
            Message::Interested => {},
            Message::NotInterested => {},
            Message::Have(_) => {},
            Message::Bitfield(_) => {},
            Message::Request(_) => {},
            Message::Piece(_) => {},
            Message::Cancel(_) => {},
        };

        Ok(())
    }
}

pub(crate) async fn handshake(
    stream: &mut TcpStream,
    id: &[u8; 20],
    file_hash: &[u8; 20],
) -> Result<(), ()> {
    let err = |_| ();

    stream.write_all(PROTOCOL).await.map_err(err)?;
    stream.write_all(&[0u8; 8]).await.map_err(err)?;
    // TODO sha1...
    stream.write_all(&[0u8; 20]).await.map_err(err)?;
    stream.write_all(id).await.map_err(err)?;
    stream.flush().await.map_err(err)?;

    let mut buf = [0u8; 20 + 8 + 20 + 20];
    stream.read_exact(&mut buf).await.map_err(err)?;

    let (protocol, flags, their_hash, their_id) =
        (&buf[0..20], &buf[20..28], &buf[28..48], &buf[48..68]);

    if protocol != PROTOCOL {
        return Err(());
    }

    if flags != &[0u8; 8] {
        return Err(());
    }

    // TODO check hash

    // TODO check id

    Ok(())
}