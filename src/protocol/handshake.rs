use std::io;
use std::time::Duration;

use async_std::io::prelude::WriteExt;
use async_std::io::timeout;
use async_std::net::TcpStream;
use futures::AsyncReadExt;
use futures::Future;

pub const PROTOCOL: &[u8; 20] = b"\x13BitTorrent protocol";

async fn t<F, T>(fut: F) -> io::Result<T>
    where
        F: Future<Output=io::Result<T>>,
{
    timeout(Duration::from_secs(30), fut).await
}

pub async fn handshake(
    stream: &mut TcpStream,
    id: &[u8],
    file_hash: &[u8],
) -> Result<(), &'static str> {
    t(stream.write(PROTOCOL))
        .await
        .map_err(|_| "failed to write protocol string")?;
    t(stream.write(&[0u8; 8]))
        .await
        .map_err(|_| "failed to write extension flags")?;
    t(stream.write(file_hash))
        .await
        .map_err(|_| "failed to write file hash")?;
    t(stream.write(id))
        .await
        .map_err(|_| "failed to write id")?;

    let mut buf = [0u8; 20 + 8 + 20 + 20];
    t(stream.read_exact(&mut buf))
        .await
        .map_err(|_| "failed to read handshake")?;

    let (protocol, _flags, their_hash, _their_id) =
        (&buf[0..20], &buf[20..28], &buf[28..48], &buf[48..68]);

    if protocol != PROTOCOL {
        return Err("protocol mismatch");
    }

    if file_hash != their_hash {
        return Err("file_hash mismatch");
    }

    Ok(())
}
