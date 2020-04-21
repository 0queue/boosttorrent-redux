use std::time::Duration;

use async_std::future::timeout;
use async_std::sync::Arc;
use async_std::sync::RwLock;
use flume::Receiver;

use crate::bitvec_ext::BitVecExt;
use crate::broadcast;
use crate::data::DownloadedPiece;
use crate::data::Lifecycle;
use crate::data::SharedState;

pub async fn writer(
    piece_length: i64,
    num_pieces: usize,
    file_length: usize,
    mut done_rx: Receiver<DownloadedPiece>,
    endgame_tx: broadcast::Sender<usize>,
    work_rx: async_std::sync::Receiver<usize>,
    shared_state: SharedState,
    haves: Arc<RwLock<Vec<usize>>>,
) -> Vec<u8> {
    println!("Starting finished work receiver");
    let mut bitfield = bit_vec::BitVec::from_elem(num_pieces, false);
    let mut output = vec![0u8; file_length];
    loop {
        match done_rx.recv_async().await {
            Ok(p) => {
                if !bitfield[p.index] {
                    let start = p.index * piece_length as usize;
                    let end = start + p.data.len();
                    output[start..end].copy_from_slice(&p.data);

                    // update state
                    bitfield.set(p.index, true);
                    haves.write().await.push(p.index);

                    let lifecycle = {
                        let mut write = shared_state.write().await;
                        write.received += 1;
                        if write.received == num_pieces {
                            write.lifecycle = Lifecycle::Done;
                        } else if work_rx.len() < 10 && write.lifecycle == Lifecycle::Downloading {
                            write.lifecycle = Lifecycle::Endgame;
                            println!("Endgame triggered");
                            println!("  work_rx.len(): {}", work_rx.len());
                            println!("  zeroes: {:?}", bitfield.zeroes());
                            endgame_tx.random_send_all(&bitfield.zeroes());
                        }

                        write.lifecycle
                    };

                    let ones = bitfield.ones().len();
                    let percent = ones as f32 / num_pieces as f32;
                    println!(
                        "Finished with {:?}. Completed: {} / {} = {}",
                        p.index, ones, num_pieces, percent
                    );
                } else {
                    // println!("Received duplicate: {}", p.index);
                }
            }
            Err(_) => {
                println!("all senders disconnected");
                break;
            }
        }
    }

    println!("Done (zeroes: {:?})", bitfield.zeroes());

    output
}
