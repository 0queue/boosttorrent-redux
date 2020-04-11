use std::io::Read;

use percent_encoding::NON_ALPHANUMERIC;
use percent_encoding::percent_encode;
use sha1::Digest;
use sha1::Sha1;

use crate::bencode::BVal;
use crate::bencode::de::deserialize;

#[allow(dead_code)]
pub enum Event {
    Started,
    Completed,
    Stopped,
}

impl std::fmt::Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Event::Started => "started",
            Event::Completed => "completed",
            Event::Stopped => "stopped",
        };

        write!(f, "{}", s)
    }
}

pub fn announce(torrent: &BVal, id: &[u8; 20], port: u16, event: Event) -> BVal {
    println!("announce: {:?}", torrent.get("announce").string());

    let info_hash = {
        let bencoded = torrent.get("info").serialize();
        let mut hasher = Sha1::new();
        hasher.input(&bencoded);
        let hash = hasher.result();
        percent_encode(&hash, NON_ALPHANUMERIC).to_string()
    };

    let peer_id = percent_encode(id, NON_ALPHANUMERIC).to_string();

    let mut request = ureq::get(&torrent.get("announce").string());

    let left = torrent.get("info").get("length").integer();

    request
        .query("info_hash", &info_hash)
        .query("peer_id", &peer_id)
        .query("port", &port.to_string())
        .query("event", &event.to_string())
        .query("compact", "1")
        .query("uploaded", "0")
        .query("downloaded", "0")
        .query("left", &left.to_string());

    println!("URL: {:?}", request);

    let response = request.call();

    let response = {
        let mut buf = Vec::new();
        response.into_reader().read_to_end(&mut buf).unwrap();

        let raw_response = String::from_utf8_lossy(&buf);

        deserialize(&buf).map_err(|_| raw_response).unwrap()
    };

    println!("Response: {:?}", response);

    response
}
