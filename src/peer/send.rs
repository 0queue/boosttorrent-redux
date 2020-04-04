use async_std::net::TcpStream;
use flume::Receiver;

use futures::io::WriteHalf;

use crate::peer::protocol::Message;

pub async fn sender(mut stream: WriteHalf<TcpStream>, mut msg_rx: Receiver<Message>) {
    loop {
        let msg = match msg_rx.recv_async().await {
            Ok(m) => m,
            Err(_) => return,
        };

        if let Err(_) = msg.send(&mut stream).await {
            return;
        }
    }
}
