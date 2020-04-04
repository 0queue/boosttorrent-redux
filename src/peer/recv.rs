use async_std::net::TcpStream;
use flume::Sender;

use futures::io::ReadHalf;

use crate::peer::protocol::Message;

pub async fn receiver(mut stream: ReadHalf<TcpStream>, msg_tx: Sender<Message>) {
    loop {
        let msg = match Message::from(&mut stream).await {
            Ok(m) => m,
            Err(_) => return,
        };

        if let Err(_) = msg_tx.send(msg) {
            return;
        }
    }
}
