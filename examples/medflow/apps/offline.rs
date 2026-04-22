//! Offline-first operation queue (prototype).
//!
//! The hospital is a place where network flakiness is real — reception
//! PCs die, the wifi drops on the third floor, the local SIM runs out
//! of data during a house call. This module lets a client (a future
//! tablet app, a CLI sync job, a server-side deferred-write worker)
//!
//!   1. ENQUEUE workflow actions against a local JSON file
//!      [`OfflineQueue::enqueue`] — runs with zero network,
//!   2. REPLAY them later against the live API when connectivity
//!      returns ([`sync_pending_operations`]),
//!   3. KEEP failed operations for inspection rather than dropping
//!      them.
//!
//! The queue is deliberately transport-agnostic — it knows nothing
//! about `reqwest`, `hyper`, auth tokens, or retry policy. The caller
//! passes a closure (`send`) that does one HTTP POST and reports
//! back. This keeps the queue testable with a deterministic stub and
//! lets the real client pick whichever HTTP library it wants.
//!
//! The on-disk format is a single JSON file; every enqueue / sync
//! pass rewrites it atomically (temp file + rename).

#![allow(dead_code)]

use chrono::{DateTime, Utc};
use rustio_core::Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

// ═══════════════════════════════════════════════════════════════
// Operation record
// ═══════════════════════════════════════════════════════════════

/// Lifecycle status of a queued operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind")]
pub enum OpStatus {
    /// Has not been sent. `sync_pending_operations` picks these up.
    Pending,
    /// Last send rejected. Stays in the queue; only an explicit
    /// [`OfflineQueue::retry_failed`] flips it back to `Pending`.
    Failed {
        reason: String,
        failed_at: DateTime<Utc>,
    },
}

/// One queued workflow action. The JSON representation keeps the
/// four fields the spec requires (`type`, `appointment_id`,
/// `timestamp`, `payload`) plus a local `id`, `status`, and
/// `attempts` — the latter three are what let the sync pass be
/// safe, idempotent from the caller's perspective, and observable
/// for operators.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    /// Local monotonic id, per queue file. Not related to server ids.
    pub id: u64,
    /// Matches a service function name:
    /// `schedule_appointment | confirm_appointment | check_in_appointment
    /// | start_consultation | complete_appointment | cancel_appointment`.
    #[serde(rename = "type")]
    pub op_type: String,
    /// Server-side id of the target appointment (if already assigned).
    /// `None` for a `schedule_appointment` operation — the id is
    /// minted by the server on successful replay.
    pub appointment_id: Option<i64>,
    /// When the client enqueued this op.
    pub timestamp: DateTime<Utc>,
    /// Free-form body the client wants the API to consume on replay.
    /// For transitions it is typically `{}`; for `schedule_appointment`
    /// it carries the full `ScheduleAppointmentRequest` shape.
    pub payload: Value,
    #[serde(default = "default_status")]
    pub status: OpStatus,
    #[serde(default)]
    pub attempts: u32,
}

fn default_status() -> OpStatus {
    OpStatus::Pending
}

// ═══════════════════════════════════════════════════════════════
// Queue — file-backed, single process, no locks
// ═══════════════════════════════════════════════════════════════

/// On-disk file shape. `next_id` preserves monotonicity even after
/// successful ops are removed from `operations`, so ids never collide
/// across a succeed-then-enqueue sequence.
#[derive(Debug, Default, Serialize, Deserialize)]
struct QueueFile {
    next_id: u64,
    operations: Vec<Operation>,
}

pub struct OfflineQueue {
    path: PathBuf,
    file: QueueFile,
}

impl OfflineQueue {
    /// Open the queue at `path`. A missing file is treated as an
    /// empty queue (first-run); a malformed file is an error — we do
    /// NOT silently truncate, because that would lose queued work.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, Error> {
        let path = path.into();
        let file = match fs::read_to_string(&path) {
            Ok(s) if s.trim().is_empty() => QueueFile::default(),
            Ok(s) => serde_json::from_str::<QueueFile>(&s)
                .map_err(|e| Error::Internal(format!("queue parse {path:?}: {e}")))?,
            Err(e) if e.kind() == ErrorKind::NotFound => QueueFile::default(),
            Err(e) => {
                return Err(Error::Internal(format!("queue open {path:?}: {e}")));
            }
        };
        Ok(Self { path, file })
    }

    /// Append a new operation. The ids are strictly monotonic within
    /// one queue file; they are NOT server ids.
    pub fn enqueue(
        &mut self,
        op_type: &str,
        appointment_id: Option<i64>,
        payload: Value,
    ) -> Result<u64, Error> {
        self.file.next_id += 1;
        let op = Operation {
            id: self.file.next_id,
            op_type: op_type.to_string(),
            appointment_id,
            timestamp: Utc::now(),
            payload,
            status: OpStatus::Pending,
            attempts: 0,
        };
        self.file.operations.push(op);
        self.persist()?;
        Ok(self.file.next_id)
    }

    /// Flip every `Failed` operation back to `Pending` so the next
    /// `sync_pending_operations` re-attempts them. Returns the
    /// number that moved.
    pub fn retry_failed(&mut self) -> Result<usize, Error> {
        let mut count = 0;
        for op in self.file.operations.iter_mut() {
            if matches!(op.status, OpStatus::Failed { .. }) {
                op.status = OpStatus::Pending;
                count += 1;
            }
        }
        if count > 0 {
            self.persist()?;
        }
        Ok(count)
    }

    pub fn operations(&self) -> &[Operation] {
        &self.file.operations
    }

    pub fn pending_count(&self) -> usize {
        self.file
            .operations
            .iter()
            .filter(|o| matches!(o.status, OpStatus::Pending))
            .count()
    }

    pub fn failed_count(&self) -> usize {
        self.file
            .operations
            .iter()
            .filter(|o| matches!(o.status, OpStatus::Failed { .. }))
            .count()
    }

    pub fn len(&self) -> usize {
        self.file.operations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.file.operations.is_empty()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Atomic write: serialise to a sibling `*.tmp`, then rename. A
    /// crash mid-write leaves either the old queue intact or the new
    /// one complete — never a half-written file.
    fn persist(&self) -> Result<(), Error> {
        let body = serde_json::to_string_pretty(&self.file)
            .map_err(|e| Error::Internal(format!("queue serialize: {e}")))?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, body)
            .map_err(|e| Error::Internal(format!("queue write {tmp:?}: {e}")))?;
        fs::rename(&tmp, &self.path)
            .map_err(|e| Error::Internal(format!("queue rename: {e}")))?;
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════
// Sync
// ═══════════════════════════════════════════════════════════════

/// Outcome summary of one [`sync_pending_operations`] pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncReport {
    /// Pending ops considered this pass.
    pub attempted: usize,
    /// Accepted by the API and removed from the queue.
    pub succeeded: usize,
    /// Rejected by the API; marked `Failed` and kept for inspection.
    pub failed: usize,
}

/// Replay every `Pending` operation in `queue` by invoking `send(op)`
/// once per op. Failed-status ops are left alone — the caller must
/// explicitly [`OfflineQueue::retry_failed`] to re-try them.
///
/// Behaviour per operation result:
///
///   * `Ok(())` — operation is removed from the queue.
///   * `Err(reason)` — operation's status becomes
///     `OpStatus::Failed { reason, failed_at }`; the op stays in the
///     file; `attempts` is incremented.
///
/// The queue file is persisted once, at the end of the pass. If the
/// process is killed mid-pass, any already-successful ops that
/// weren't yet written will be replayed on the next pass — the API
/// layer is expected to handle the duplicate case on its own
/// (lifecycle self-edges are valid no-ops for most status
/// transitions; `schedule_appointment` genuinely creates a duplicate
/// — a per-op idempotency key on the server is the proper fix and
/// is out of scope for this prototype).
pub async fn sync_pending_operations<F, Fut>(
    queue: &mut OfflineQueue,
    mut send: F,
) -> Result<SyncReport, Error>
where
    F: FnMut(&Operation) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let mut attempted = 0usize;
    let mut succeeded = 0usize;
    let mut failed = 0usize;

    // Consume the operations by value, build up the post-pass vec.
    // Order is preserved: failures keep their place, successes drop.
    let prior = std::mem::take(&mut queue.file.operations);
    let mut kept: Vec<Operation> = Vec::with_capacity(prior.len());

    for op in prior {
        if !matches!(op.status, OpStatus::Pending) {
            // Keep Failed rows untouched — retry is explicit.
            kept.push(op);
            continue;
        }
        attempted += 1;
        match send(&op).await {
            Ok(()) => {
                succeeded += 1;
                eprintln!(
                    "[offline/sync] op #{} {:>24}  succeeded",
                    op.id, op.op_type,
                );
            }
            Err(reason) => {
                failed += 1;
                eprintln!(
                    "[offline/sync] op #{} {:>24}  failed: {}",
                    op.id, op.op_type, reason,
                );
                let mut failed_op = op;
                failed_op.status = OpStatus::Failed {
                    reason,
                    failed_at: Utc::now(),
                };
                failed_op.attempts += 1;
                kept.push(failed_op);
            }
        }
    }

    queue.file.operations = kept;
    queue.persist()?;

    Ok(SyncReport {
        attempted,
        succeeded,
        failed,
    })
}
