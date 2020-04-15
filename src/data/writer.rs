use async_std::fs::File;
use async_std::io::SeekFrom;
use flume::Receiver;
use futures::AsyncSeekExt;
use futures::AsyncWriteExt;

use crate::count_ones;
use crate::data::DownloadedPiece;
use crate::data::SharedState;

pub async fn writer(
    mut output: File,
    piece_length: i64,
    mut done_rx: Receiver<DownloadedPiece>,
    num_pieces: usize,
    shared_state: SharedState,
) {
    println!("Starting finished work receiver");
    let mut bitfield = bit_vec::BitVec::from_elem(num_pieces, false);
    loop {
        match done_rx.recv_async().await {
            Ok(p) => {
                if !bitfield[p.index] {
                    output
                        .seek(SeekFrom::Start((p.index * piece_length as usize) as u64))
                        .await
                        .unwrap();
                    output.write_all(&p.data).await.unwrap();
                    bitfield.set(p.index, true);
                    shared_state.write().await.received += 1;
                    if shared_state.read().await.received == num_pieces {
                        shared_state.write().await.done = true;
                    }

                    if num_pieces - shared_state.read().await.received < 5 {
                        println!(
                            "endgame missing: {:?}",
                            bitfield
                                .iter()
                                .enumerate()
                                .filter(|(_, b)| !*b)
                                .collect::<Vec<_>>()
                        );
                    }

                    let ones = count_ones(&bitfield);
                    let percent = ones as f32 / num_pieces as f32;
                    println!(
                        "Finished with {:?}. Completed: {} / {} = {}",
                        p.index, ones, num_pieces, percent
                    );
                } else {
                    println!("Received duplicate: {}", p.index);
                }
            }
            Err(_) => {
                println!("all senders disconnected");
                break;
            }
        }
    }

    output.sync_all().await.unwrap();
    println!("Done");
}