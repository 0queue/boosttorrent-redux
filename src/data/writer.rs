use async_std::fs::File;
use async_std::io::SeekFrom;
use flume::Receiver;
use futures::AsyncSeekExt;
use futures::AsyncWriteExt;

use crate::count_ones;
use crate::data::DownloadedPiece;
use crate::data::Lifecycle;
use crate::data::SharedState;

pub async fn writer(
    mut output: File,
    piece_length: i64,
    num_pieces: usize,
    mut done_rx: Receiver<DownloadedPiece>,
    shared_state: SharedState,
) {
    println!("Starting finished work receiver");
    let mut bitfield = bit_vec::BitVec::from_elem(num_pieces, false);
    loop {
        match done_rx.recv_async().await {
            Ok(p) => {
                if !bitfield[p.index] {
                    // write to disk
                    output
                        .seek(SeekFrom::Start((p.index * piece_length as usize) as u64))
                        .await
                        .unwrap();
                    output.write_all(&p.data).await.unwrap();

                    // update state
                    bitfield.set(p.index, true);
                    let lifecycle = {
                        let mut write = shared_state.write().await;
                        write.received += 1;
                        if write.received == num_pieces {
                            write.lifecycle = Lifecycle::Done;
                        } else if num_pieces - write.received < 10 {
                            write.lifecycle = Lifecycle::Endgame;
                        }

                        write.lifecycle
                    };

                    if lifecycle == Lifecycle::Endgame {
                        // TODO start a task to take from work_queue and broadcast
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
