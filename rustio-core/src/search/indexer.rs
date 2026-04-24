//! Async indexer. Writes arrive on a bounded channel; a background
//! task drains and bulk-indexes them.
//!
//! Why batching: Meilisearch takes ~1ms to index a single document and
//! ~5ms to index a batch of 1000. If you POST every row the moment it's
//! created, a busy write endpoint gets crushed by network RTT. Batching
//! brings the per-doc cost down to microseconds.
//!
//! Backpressure: the channel has a fixed capacity (default 1024). If
//! the indexer falls behind, `queue()` blocks. That's intentional — it
//! means the write path can't scribble data into the pipeline faster
//! than it can be indexed.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};

use crate::error::Result;

use super::client::MeiliClient;

/// A single write to propagate to the search index.
#[derive(Debug, Clone)]
pub enum IndexJob {
    Upsert {
        index: String,
        primary_key: String,
        document: Value,
    },
    Delete {
        index: String,
        id: String,
    },
}

/// Handle to the background indexer. Cheap to clone.
#[derive(Clone)]
pub struct Indexer {
    tx: mpsc::Sender<IndexJob>,
}

impl Indexer {
    /// Spawn the worker. Returns an `Indexer` you clone into anywhere
    /// that writes data (handlers, background tasks, migrations).
    pub fn spawn(client: Arc<MeiliClient>, capacity: usize) -> Self {
        let (tx, rx) = mpsc::channel(capacity);
        tokio::spawn(run_worker(client, rx));
        Self { tx }
    }

    /// Queue a job. Blocks if the channel is full.
    pub async fn queue(&self, job: IndexJob) -> Result<()> {
        self.tx
            .send(job)
            .await
            .map_err(|e| crate::error::Error::Internal(format!("indexer queue: {e}")))
    }

    /// Fire-and-forget version. Used by the admin writes — we don't
    /// want a stalled indexer to turn a user-visible POST into an error.
    pub fn queue_detached(&self, job: IndexJob) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Err(e) = tx.send(job).await {
                log::warn!("indexer queue dropped: {e}");
            }
        });
    }
}

async fn run_worker(client: Arc<MeiliClient>, mut rx: mpsc::Receiver<IndexJob>) {
    // Batching window: drain up to 500 pending jobs or 100ms, whichever
    // comes first. Good balance between responsiveness (someone adds a
    // post and wants it searchable "right now") and throughput (bulk
    // index beats single-doc index by ~10×).
    let mut batch: Vec<IndexJob> = Vec::with_capacity(512);
    let mut tick = interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            maybe_job = rx.recv() => {
                match maybe_job {
                    Some(job) => {
                        batch.push(job);
                        // Greedy drain — take whatever else is immediately ready.
                        while batch.len() < 500 {
                            match rx.try_recv() {
                                Ok(more) => batch.push(more),
                                Err(_) => break,
                            }
                        }
                    }
                    None => {
                        // Channel closed; flush and exit.
                        flush(&client, &mut batch).await;
                        break;
                    }
                }
            }
            _ = tick.tick() => {
                if !batch.is_empty() {
                    flush(&client, &mut batch).await;
                }
            }
        }
    }
}

async fn flush(client: &MeiliClient, batch: &mut Vec<IndexJob>) {
    if batch.is_empty() {
        return;
    }
    // Group by (index, op). Upserts can go as bulk; deletes are per-id.
    let mut upserts: std::collections::HashMap<(String, String), Vec<Value>> =
        std::collections::HashMap::new();
    let mut deletes: Vec<(String, String)> = Vec::new();

    for job in batch.drain(..) {
        match job {
            IndexJob::Upsert { index, primary_key, document } => {
                upserts.entry((index, primary_key)).or_default().push(document);
            }
            IndexJob::Delete { index, id } => {
                deletes.push((index, id));
            }
        }
    }

    for ((index, primary_key), docs) in upserts {
        let count = docs.len();
        if let Err(e) = client.add_documents(&index, &docs, &primary_key).await {
            log::warn!("indexer upsert failed ({index}, {count} docs): {e}");
        } else {
            log::debug!("indexer upserted {count} docs into {index}");
        }
    }

    for (index, id) in deletes {
        if let Err(e) = client.delete_document(&index, &id).await {
            log::warn!("indexer delete failed ({index}, {id}): {e}");
        }
    }
}
