//! HTTP API layer — appointment endpoints.
//!
//! Every handler is a thin pass-through that
//!
//!   1. parses and validates its input DTO,
//!   2. dispatches to exactly one [`super::services`] function,
//!   3. serialises the result (or error) as JSON.
//!
//! No business logic lives here. No direct DB access. No workflow
//! orchestration. If a rule needs to change, it changes in
//! `services.rs`, not here.
//!
//! ## Route design
//!
//! Action-based, not CRUD. The resource is the appointment; the
//! action is the state transition:
//!
//!   POST /api/appointments                      → schedule_appointment
//!   POST /api/appointments/:id/confirm          → confirm_appointment
//!   POST /api/appointments/:id/check-in         → check_in_appointment
//!   POST /api/appointments/:id/start            → start_consultation
//!   POST /api/appointments/:id/complete         → complete_appointment
//!   POST /api/appointments/:id/cancel           → cancel_appointment
//!
//! Medical- and billing-related endpoints are reserved for the next
//! iteration; their service functions already exist and are ready to
//! be wired.
//!
//! ## Authentication — deliberately absent
//!
//! No auth is wired in this iteration. When it arrives it goes in
//! [`require_actor`] below (currently a no-op returning `Ok(())`)
//! and as middleware on the api sub-router. Handlers should keep
//! calling `require_actor(&req).await?` at the top so the eventual
//! wiring is a single-point edit, not a handler-by-handler rewrite.
//!
//! ## Error mapping
//!
//! | service / infra error             | HTTP status | body shape              |
//! | --------------------------------- | ----------- | ----------------------- |
//! | `Error::BadRequest(msg)`          | 400         | `{"error": <msg>}`      |
//! | `Error::NotFound`                 | 404         | `{"error": "not_found"}`|
//! | `Error::Internal(msg)` w/ UNIQUE  | 409         | `{"error": <msg>}`      |
//! | `Error::Internal(msg)` w/ FOREIGN | 400         | `{"error": <msg>}`      |
//! | other `Error::Internal`           | 500         | `{"error": <msg>}`      |
//! | `Error::PayloadTooLarge`          | 413         | `{"error": ...}`        |

#![allow(dead_code)] // callable once a `Server` is pointed at the router.

use bytes::Bytes;
use chrono::{DateTime, Utc};
use http_body_util::{BodyExt, Full, Limited};
use rustio_core::{Db, Error, Params, Request, Response, Router};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::services;

// ═══════════════════════════════════════════════════════════════
// Route registration
// ═══════════════════════════════════════════════════════════════

/// Wire every API endpoint into the supplied router. Called from
/// `apps::register_all`. The `Db` is cloned into each handler's
/// closure so handlers own their own handle without sharing mutable
/// state across threads.
pub fn register(mut router: Router, db: &Db) -> Router {
    // --- Appointments ---
    router = router.post("/api/appointments", {
        let db = db.clone();
        move |req, _params| {
            let db = db.clone();
            async move { dispatch(schedule(&db, req).await) }
        }
    });

    router = router.post("/api/appointments/:id/confirm", {
        let db = db.clone();
        move |req, params| {
            let db = db.clone();
            async move {
                dispatch(
                    run_transition(&db, &params, req, |db, id| {
                        Box::pin(services::confirm_appointment(db, id))
                    })
                    .await,
                )
            }
        }
    });

    router = router.post("/api/appointments/:id/check-in", {
        let db = db.clone();
        move |req, params| {
            let db = db.clone();
            async move { dispatch(check_in(&db, &params, req).await) }
        }
    });

    router = router.post("/api/appointments/:id/start", {
        let db = db.clone();
        move |req, params| {
            let db = db.clone();
            async move {
                dispatch(
                    run_transition(&db, &params, req, |db, id| {
                        Box::pin(services::start_consultation(db, id))
                    })
                    .await,
                )
            }
        }
    });

    router = router.post("/api/appointments/:id/complete", {
        let db = db.clone();
        move |req, params| {
            let db = db.clone();
            async move {
                dispatch(
                    run_transition(&db, &params, req, |db, id| {
                        Box::pin(services::complete_appointment(db, id))
                    })
                    .await,
                )
            }
        }
    });

    router = router.post("/api/appointments/:id/cancel", {
        let db = db.clone();
        move |req, params| {
            let db = db.clone();
            async move {
                dispatch(
                    run_transition(&db, &params, req, |db, id| {
                        Box::pin(services::cancel_appointment(db, id))
                    })
                    .await,
                )
            }
        }
    });

    router
}

// ═══════════════════════════════════════════════════════════════
// Handler helpers — each delegates to exactly one service fn
// ═══════════════════════════════════════════════════════════════

async fn schedule(db: &Db, req: Request) -> Result<Response, Error> {
    require_actor(&req).await?;
    let body: ScheduleAppointmentRequest = read_json(req).await?;
    let input = services::ScheduleAppointmentInput {
        patient_id: body.patient_id,
        doctor_id: body.doctor_id,
        department_id: body.department_id,
        scheduled_at: body.scheduled_at,
        reason: body.reason,
        notes: body.notes,
        duration_minutes: body.duration_minutes,
        priority: body.priority,
    };
    let id = services::schedule_appointment(db, input).await?;
    Ok(json_response(201, &IdResponse { id }))
}

/// Dispatch a lifecycle transition to one of the four
/// `services::{confirm,start_consultation,complete,cancel}_appointment`
/// functions. Each has the same shape — `(&Db, i64) -> Result<(), Error>` —
/// so one helper handles them all. The caller passes a boxed future
/// builder because the higher-ranked lifetime on the `&Db` parameter
/// makes a plain `Fn(&Db, i64) -> impl Future` hard to satisfy with
/// bare service-function references.
async fn run_transition<'a, F>(
    db: &'a Db,
    params: &'a Params,
    req: Request,
    service_fn: F,
) -> Result<Response, Error>
where
    F: FnOnce(
        &'a Db,
        i64,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), Error>> + Send + 'a>,
    >,
{
    require_actor(&req).await?;
    let id = parse_id(params)?;
    service_fn(db, id).await?;
    Ok(json_response(200, &OkResponse { ok: true }))
}

async fn check_in(db: &Db, params: &Params, req: Request) -> Result<Response, Error> {
    require_actor(&req).await?;
    let id = parse_id(params)?;
    let body: CheckInRequest = read_json(req).await?;
    let input = services::CheckInInput {
        appointment_id: id,
        staff_id: body.staff_id,
        room_id: body.room_id,
        priority: body.priority.unwrap_or(5),
        notes: body.notes.unwrap_or_default(),
    };
    let check_in_id = services::check_in_appointment(db, input).await?;
    Ok(json_response(
        201,
        &IdResponse { id: check_in_id },
    ))
}

// ═══════════════════════════════════════════════════════════════
// Request / Response DTOs
// ═══════════════════════════════════════════════════════════════

#[derive(Deserialize)]
struct ScheduleAppointmentRequest {
    patient_id: i64,
    doctor_id: i64,
    #[serde(default)]
    department_id: Option<i64>,
    scheduled_at: DateTime<Utc>,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    notes: String,
    duration_minutes: i32,
    #[serde(default = "default_priority")]
    priority: i32,
}

#[derive(Deserialize)]
struct CheckInRequest {
    #[serde(default)]
    staff_id: Option<i64>,
    #[serde(default)]
    room_id: Option<i64>,
    #[serde(default)]
    priority: Option<i32>,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Serialize)]
struct IdResponse {
    id: i64,
}

#[derive(Serialize)]
struct OkResponse {
    ok: bool,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
}

fn default_priority() -> i32 {
    5
}

// ═══════════════════════════════════════════════════════════════
// Request parsing / response construction
// ═══════════════════════════════════════════════════════════════

/// Maximum JSON body accepted by any API handler. Enforced via
/// [`http_body_util::Limited`] so a client streaming an unbounded
/// body cannot hold a worker indefinitely.
const MAX_JSON_BODY: usize = 1024 * 1024; // 1 MiB

/// Collect the request body and parse it as the caller's DTO. Errors
/// surface as `Error::BadRequest` for parse failures and
/// `Error::PayloadTooLarge` for oversized uploads — both mapped to
/// their correct HTTP status via [`dispatch`].
async fn read_json<T: DeserializeOwned>(req: Request) -> Result<T, Error> {
    let (_, body, _) = req.into_parts();
    let collected = Limited::new(body, MAX_JSON_BODY)
        .collect()
        .await
        .map_err(|e| {
            if e.downcast_ref::<http_body_util::LengthLimitError>()
                .is_some()
            {
                Error::PayloadTooLarge
            } else {
                Error::BadRequest(format!("failed to read request body: {e}"))
            }
        })?;
    let bytes = collected.to_bytes();
    serde_json::from_slice(&bytes).map_err(|e| Error::BadRequest(format!("invalid JSON: {e}")))
}

/// Extract the `:id` path parameter as an i64. 404 when missing, 400
/// when present but not an integer.
fn parse_id(params: &Params) -> Result<i64, Error> {
    let raw = params
        .get("id")
        .ok_or_else(|| Error::BadRequest("missing :id path parameter".into()))?;
    raw.parse::<i64>()
        .map_err(|_| Error::BadRequest(format!("invalid id: `{raw}`")))
}

/// Build a JSON response with an arbitrary status code. The only
/// reason this exists alongside `rustio_core::json_raw` is that the
/// core helper is hard-coded to `200 OK`; the API needs 201 for
/// creates, 400 / 404 / 409 for errors.
fn json_response<T: Serialize>(status: u16, body: &T) -> Response {
    let body_str = serde_json::to_string(body).unwrap_or_else(|_| "{}".to_string());
    hyper::Response::builder()
        .status(status)
        .header("content-type", "application/json; charset=utf-8")
        .body(Full::new(Bytes::from(body_str)))
        .expect("valid response")
}

// ═══════════════════════════════════════════════════════════════
// Error → HTTP mapping
// ═══════════════════════════════════════════════════════════════

/// Convert a `Result<Response, Error>` into a `Result<Response, Error>`
/// where every `Err` has been mapped to a JSON-bodied response. The
/// router already has a final-safety-net converter for uncaught
/// `Err`, but its default produces `text/plain` — APIs want JSON.
fn dispatch(result: Result<Response, Error>) -> Result<Response, Error> {
    match result {
        Ok(resp) => Ok(resp),
        Err(err) => Ok(error_response(err)),
    }
}

fn error_response(err: Error) -> Response {
    let (status, message) = classify(err);
    json_response(status, &ErrorBody { error: &message })
}

/// Map a service-layer [`Error`] to an HTTP status and a user-safe
/// message. FK violations and UNIQUE violations surface as sqlx
/// errors wrapped in [`Error::Internal`]; sniff the message to
/// re-classify them as 400 / 409 instead of leaking "internal".
fn classify(err: Error) -> (u16, String) {
    match err {
        Error::NotFound => (404, "not_found".to_string()),
        Error::BadRequest(msg) => (400, msg),
        Error::Unauthorized => (401, "unauthorized".to_string()),
        Error::Forbidden => (403, "forbidden".to_string()),
        Error::PayloadTooLarge => (413, "payload_too_large".to_string()),
        Error::TooManyRequests => (429, "too_many_requests".to_string()),
        Error::MethodNotAllowed => (405, "method_not_allowed".to_string()),
        Error::Internal(msg) => {
            if msg.contains("UNIQUE") || msg.contains("unique") {
                (409, msg)
            } else if msg.contains("FOREIGN KEY") || msg.contains("foreign key") {
                (400, msg)
            } else {
                (500, msg)
            }
        }
        // `Error` is `#[non_exhaustive]`; new variants default to 500.
        _ => (500, "internal_error".to_string()),
    }
}

// ═══════════════════════════════════════════════════════════════
// Authentication hook — intentionally a no-op for now
// ═══════════════════════════════════════════════════════════════

/// Placeholder for the per-handler actor check. When auth lands it
/// will read `req.ctx().get::<Identity>()`, validate the session /
/// token, and attach the actor id to the request context for service
/// functions that need it. For now: every caller is permitted.
///
/// Keep calling this from every handler so the eventual wiring is a
/// one-file change.
async fn require_actor(_req: &Request) -> Result<(), Error> {
    Ok(())
}
