use std::io::Read;

use percent_encoding::NON_ALPHANUMERIC;
use percent_encoding::percent_encode;

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
    let info_hash = percent_encode(&torrent["info"].hash(), NON_ALPHANUMERIC).to_string();

    let peer_id = percent_encode(id, NON_ALPHANUMERIC).to_string();
    let left = torrent["info"]["length"].integer();

    let mut request = ureq::get(&torrent["announce"].string());
    request
        .query("info_hash", &info_hash)
        .query("peer_id", &peer_id)
        .query("port", &port.to_string())
        .query("event", &event.to_string())
        .query("compact", "1")
        .query("uploaded", "0")
        .query("downloaded", "0")
        .query("left", &left.to_string());

    println!("Announcing: {:?}", request);

    let response = {
        let mut buf = Vec::new();
        request.call().into_reader().read_to_end(&mut buf).unwrap();
        let raw_response = String::from_utf8_lossy(&buf);
        deserialize(&buf).map_err(|_| raw_response).unwrap()
    };

    response
}
