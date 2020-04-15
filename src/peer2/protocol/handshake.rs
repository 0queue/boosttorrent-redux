use std::time::Duration;

use async_std::io::prelude::WriteExt;
use async_std::net::TcpStream;
use futures::AsyncReadExt;

use crate::peer2::protocol::PROTOCOL;
use crate::peer2::Us;

// use crate::peer::protocol::PROTOCOL;
// use crate::peer::Us;

pub async fn handshake(
    stream: &mut TcpStream,
    us: &Us,
) -> Result<(), &'static str> {
    stream
        .write(PROTOCOL)
        .await
        .map_err(|_| "failed to write protocol string")?;
    stream
        .write(&[0u8; 8])
        .await
        .map_err(|_| "failed to write extension flags")?;
    stream
        .write(&us.file_hash)
        .await
        .map_err(|_| "failed to write file hash")?;

    stream.write(&us.id).await.map_err(|_| "failed to write id")?;

    let mut buf = [0u8; 20 + 8 + 20 + 20];
    async_std::io::timeout(Duration::from_secs(5), stream.read_exact(&mut buf))
        .await
        .map_err(|e| {
            eprintln!("read_exact error {:?}", e);
            "failed to read_exact"
        })?;

    let (protocol, flags, _their_hash, _their_id) =
        (&buf[0..20], &buf[20..28], &buf[28..48], &buf[48..68]);

    if protocol != PROTOCOL {
        return Err("protocol mismatch");
    }

    if flags != &[0u8; 8] {
        println!("Flags mismatch {:?}", flags);
    }

    // TODO check hash
    // TODO check id

    Ok(())
}
