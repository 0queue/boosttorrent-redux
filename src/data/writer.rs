use std::time::Duration;

use async_std::fs::File;
use async_std::future::timeout;
use async_std::future::TimeoutError;
use async_std::io::SeekFrom;
use async_std::sync::Arc;
use crossbeam::queue::SegQueue;
use flume::Receiver;
use futures::AsyncSeekExt;
use futures::AsyncWriteExt;
use futures::StreamExt;

use crate::broadcast;
use crate::count_ones;
use crate::data::DownloadedPiece;
use crate::data::Lifecycle;
use crate::data::SharedState;
use crate::PieceMeta;

pub async fn writer(
    mut output: File,
    piece_length: i64,
    num_pieces: usize,
    mut done_rx: Receiver<DownloadedPiece>,
    endgame_tx: broadcast::Sender<PieceMeta>,
    work_rx: async_std::sync::Receiver<PieceMeta>,
    shared_state: SharedState,
) {
    println!("Starting finished work receiver");
    let mut bitfield = bit_vec::BitVec::from_elem(num_pieces, false);
    let mut forward_handle: Option<_> = None;
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
                        } else if work_rx.len() < 10 && write.lifecycle == Lifecycle::Downloading {
                            write.lifecycle = Lifecycle::Endgame;
                            println!("Endgame triggered");
                            println!("  work_rx.len(): {}", work_rx.len());
                            println!("  zeroes: {:?}", zeroes(&bitfield));
                        }

                        write.lifecycle
                    };

                    if lifecycle == Lifecycle::Endgame && forward_handle.is_none() {
                        // TODO change endgame mode to start when there are x pieces remaining
                        //   right now if it detects there are ten missing, they may already
                        //   be claimed

                        // TODO receive timeouts on peers?
                        let s = shared_state.clone();
                        let w = work_rx.clone();
                        let e = endgame_tx.clone();
                        forward_handle = Some(async_std::task::spawn(async move {
                            println!("Starting endgame mode");
                            loop {
                                match timeout(Duration::from_secs(5), w.recv()).await {
                                    Ok(Some(p)) => {
                                        println!("ENDGAME PIECE: {}", p.index);
                                        e.send(p);
                                    }
                                    _ => {
                                        if s.read().await.lifecycle == Lifecycle::Done {
                                            break;
                                        }
                                    }
                                }
                            }
                        }));
                    }

                    let ones = count_ones(&bitfield);
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

    output.sync_all().await.unwrap();
    println!("Done (zeroes: {:?})", zeroes(&bitfield));
}

fn zeroes(bitfield: &bit_vec::BitVec) -> Vec<usize> {
    bitfield
        .iter()
        .enumerate()
        .filter_map(|(i, b)| if !b { Some(i) } else { None })
        .collect()
}
