//! Integration tests for the offline queue.
//!
//! Every test runs with its own temp-file path — keeps tests isolated,
//! keeps the repo clean of test artefacts, and lets the whole suite
//! run in parallel without contention.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::offline::{sync_pending_operations, OfflineQueue, OpStatus, SyncReport};

fn tmp_queue_path(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir();
    dir.join(format!(
        "medflow-offline-{tag}-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ))
}

// ═══════════════════════════════════════════════════════════════
// 1. Enqueue without network — queue persists on disk
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn enqueue_persists_without_network() {
    let path = tmp_queue_path("enqueue");
    {
        let mut q = OfflineQueue::open(&path).unwrap();
        assert!(q.is_empty());

        q.enqueue(
            "schedule_appointment",
            None,
            json!({
                "patient_id": 1,
                "doctor_id": 1,
                "scheduled_at": "2026-05-10T10:00:00Z",
                "duration_minutes": 30,
                "priority": 5,
                "reason": "Consult",
                "notes": "",
            }),
        )
        .unwrap();

        q.enqueue("confirm_appointment", Some(1), json!({})).unwrap();
        q.enqueue("cancel_appointment", Some(1), json!({})).unwrap();

        assert_eq!(q.len(), 3);
        assert_eq!(q.pending_count(), 3);
        assert_eq!(q.failed_count(), 0);
    }

    // Re-open: every op still there, ids monotonic, status Pending.
    let q2 = OfflineQueue::open(&path).unwrap();
    assert_eq!(q2.len(), 3);
    let ids: Vec<u64> = q2.operations().iter().map(|o| o.id).collect();
    assert_eq!(ids, vec![1, 2, 3]);

    let _ = std::fs::remove_file(&path);
}

// ═══════════════════════════════════════════════════════════════
// 2. JSON shape matches the spec — `type`, `appointment_id`,
//    `timestamp`, `payload` are all present and correctly named.
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn on_disk_json_has_required_fields() {
    let path = tmp_queue_path("json");
    let mut q = OfflineQueue::open(&path).unwrap();
    q.enqueue("confirm_appointment", Some(42), json!({})).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let op = &parsed["operations"][0];

    assert_eq!(op["type"], "confirm_appointment");
    assert_eq!(op["appointment_id"], 42);
    assert!(op["timestamp"].is_string());
    assert!(op["payload"].is_object());

    let _ = std::fs::remove_file(&path);
}

// ═══════════════════════════════════════════════════════════════
// 3. Sync happy path — every pending op succeeds → queue empties.
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn sync_removes_successful_operations() {
    let path = tmp_queue_path("sync-ok");
    let mut q = OfflineQueue::open(&path).unwrap();
    q.enqueue("confirm_appointment", Some(1), json!({})).unwrap();
    q.enqueue("cancel_appointment", Some(2), json!({})).unwrap();

    // Stub that "accepts" every operation.
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_clone = seen.clone();
    let report = sync_pending_operations(&mut q, move |op| {
        let seen = seen_clone.clone();
        let name = op.op_type.clone();
        async move {
            seen.lock().unwrap().push(name);
            Ok(())
        }
    })
    .await
    .unwrap();

    assert_eq!(
        report,
        SyncReport {
            attempted: 2,
            succeeded: 2,
            failed: 0
        }
    );
    assert!(q.is_empty(), "successful ops must be dropped");
    assert_eq!(
        *seen.lock().unwrap(),
        vec![
            "confirm_appointment".to_string(),
            "cancel_appointment".to_string(),
        ]
    );

    // Re-open from disk — persistence confirms the empty state.
    let q2 = OfflineQueue::open(&path).unwrap();
    assert!(q2.is_empty());

    let _ = std::fs::remove_file(&path);
}

// ═══════════════════════════════════════════════════════════════
// 4. Sync failure path — failed ops stay, reason + attempts recorded.
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn sync_keeps_failed_operations_with_reason() {
    let path = tmp_queue_path("sync-fail");
    let mut q = OfflineQueue::open(&path).unwrap();
    q.enqueue("confirm_appointment", Some(99), json!({})).unwrap();
    q.enqueue("cancel_appointment", Some(100), json!({})).unwrap();

    // Stub that rejects everything with a specific reason.
    let report = sync_pending_operations(&mut q, |_op| async {
        Err::<(), String>("API 400: appointment not found".to_string())
    })
    .await
    .unwrap();

    assert_eq!(
        report,
        SyncReport {
            attempted: 2,
            succeeded: 0,
            failed: 2
        }
    );
    assert_eq!(q.len(), 2, "failed ops must be KEPT, not dropped");
    assert_eq!(q.pending_count(), 0);
    assert_eq!(q.failed_count(), 2);

    for op in q.operations() {
        match &op.status {
            OpStatus::Failed { reason, failed_at: _ } => {
                assert!(
                    reason.contains("API 400"),
                    "reason must be preserved: {reason}"
                );
            }
            OpStatus::Pending => panic!("expected Failed, got Pending"),
        }
        assert_eq!(op.attempts, 1);
    }

    // Second pass should NOT re-try Failed ops automatically.
    let rpt = sync_pending_operations(&mut q, |_op| async {
        Err::<(), String>("should not be called".into())
    })
    .await
    .unwrap();
    assert_eq!(
        rpt,
        SyncReport {
            attempted: 0,
            succeeded: 0,
            failed: 0
        },
        "sync must skip Failed ops unless explicitly retried"
    );

    let _ = std::fs::remove_file(&path);
}

// ═══════════════════════════════════════════════════════════════
// 5. retry_failed → sync — recovery flow after transient outage.
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn retry_failed_then_sync_clears_the_queue() {
    let path = tmp_queue_path("retry");
    let mut q = OfflineQueue::open(&path).unwrap();
    q.enqueue("confirm_appointment", Some(1), json!({})).unwrap();

    // First pass: server was down → op marked Failed.
    sync_pending_operations(&mut q, |_op| async {
        Err::<(), String>("network unreachable".into())
    })
    .await
    .unwrap();
    assert_eq!(q.failed_count(), 1);

    // Operator flips Failed back to Pending.
    let moved = q.retry_failed().unwrap();
    assert_eq!(moved, 1);
    assert_eq!(q.pending_count(), 1);
    assert_eq!(q.failed_count(), 0);

    // Server came back up.
    let report = sync_pending_operations(&mut q, |_op| async { Ok(()) })
        .await
        .unwrap();
    assert_eq!(
        report,
        SyncReport {
            attempted: 1,
            succeeded: 1,
            failed: 0
        }
    );
    assert!(q.is_empty());

    let _ = std::fs::remove_file(&path);
}

// ═══════════════════════════════════════════════════════════════
// 6. Mixed outcomes — partial success in one pass.
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn sync_handles_mixed_outcomes_in_one_pass() {
    let path = tmp_queue_path("mixed");
    let mut q = OfflineQueue::open(&path).unwrap();
    q.enqueue("confirm_appointment", Some(1), json!({})).unwrap(); // will succeed
    q.enqueue("cancel_appointment", Some(2), json!({})).unwrap(); // will fail
    q.enqueue("complete_appointment", Some(3), json!({})).unwrap(); // will succeed

    let report = sync_pending_operations(&mut q, |op| {
        let is_cancel = op.op_type == "cancel_appointment";
        async move {
            if is_cancel {
                Err("API 409: already cancelled".to_string())
            } else {
                Ok(())
            }
        }
    })
    .await
    .unwrap();

    assert_eq!(
        report,
        SyncReport {
            attempted: 3,
            succeeded: 2,
            failed: 1
        }
    );
    assert_eq!(q.len(), 1, "only the failed one remains");
    let remaining = &q.operations()[0];
    assert_eq!(remaining.op_type, "cancel_appointment");
    assert!(matches!(remaining.status, OpStatus::Failed { .. }));

    let _ = std::fs::remove_file(&path);
}

// ═══════════════════════════════════════════════════════════════
// 7. Missing file → empty queue, not an error.
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn open_missing_file_starts_empty() {
    let path = tmp_queue_path("missing");
    let q = OfflineQueue::open(&path).unwrap();
    assert!(q.is_empty());
    // File should NOT have been created by open() alone — only by enqueue/sync.
    assert!(!path.exists());
}
