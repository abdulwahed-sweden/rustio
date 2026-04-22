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
//! ## Authentication — bearer tokens over the session table
//!
//! `POST /api/auth/login` validates email + password and returns an
//! opaque bearer token (the `rustio_sessions.id` — a 64-char hex
//! string). Every other handler extracts that token from the
//! `Authorization: Bearer <token>` header, looks up a valid session,
//! loads the user, and passes it into a role check.
//!
//!   * **Missing / invalid / expired token** → `401 Unauthorized`
//!   * **Valid token, wrong role for this action** → `403 Forbidden`
//!
//! Reused directly from `rustio_core::auth`:
//!
//!   * the `rustio_users` and `rustio_sessions` tables (created by
//!     `migrations::apply`),
//!   * argon2id hashing via `auth::password::verify`,
//!   * cryptographic token generation + expiry via `auth::session::create`
//!     and `auth::session::find_valid`,
//!   * the `User` struct.
//!
//! ## Role model
//!
//!   * `receptionist` — schedule / confirm / check-in / cancel
//!   * `doctor` — start / complete / cancel
//!   * `billing` — (medical + billing endpoints, not yet wired)
//!   * `admin` — super-role, passes every check
//!
//! ## Error mapping
//!
//! | service / infra error             | HTTP status | body shape              |
//! | --------------------------------- | ----------- | ----------------------- |
//! | `Error::BadRequest(msg)`          | 400         | `{"error": <msg>}`      |
//! | `Error::NotFound`                 | 404         | `{"error": "not_found"}`|
//! | `Error::Unauthorized`             | 401         | `{"error": "unauthorized"}` |
//! | `Error::Forbidden`                | 403         | `{"error": "forbidden"}` |
//! | `Error::Internal(msg)` w/ UNIQUE  | 409         | `{"error": <msg>}`      |
//! | `Error::Internal(msg)` w/ FOREIGN | 400         | `{"error": <msg>}`      |
//! | other `Error::Internal`           | 500         | `{"error": <msg>}`      |
//! | `Error::PayloadTooLarge`          | 413         | `{"error": ...}`        |

#![allow(dead_code)] // callable once a `Server` is pointed at the router.

use bytes::Bytes;
use chrono::{DateTime, Utc};
use http_body_util::{BodyExt, Full, Limited};
use rustio_core::auth::{password, session, user, User};
use rustio_core::{Db, Error, Params, Request, Response, Router};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::services;

// Role vocabulary. Must match the strings passed to
// `rustio user create --role …` when seeding API accounts.
const ROLE_RECEPTIONIST: &str = "receptionist";
const ROLE_DOCTOR: &str = "doctor";
const ROLE_BILLING: &str = "billing";
const ROLE_ADMIN: &str = "admin";

// ═══════════════════════════════════════════════════════════════
// Route registration
// ═══════════════════════════════════════════════════════════════

/// Wire every API endpoint into the supplied router. Called from
/// `apps::register_all`. The `Db` is cloned into each handler's
/// closure so handlers own their own handle without sharing mutable
/// state across threads.
pub fn register(mut router: Router, db: &Db) -> Router {
    // --- Auth ---
    router = router.post("/api/auth/login", {
        let db = db.clone();
        move |req, _params| {
            let db = db.clone();
            async move { dispatch(login(&db, req).await) }
        }
    });

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
                    run_transition(
                        &db,
                        &params,
                        req,
                        &[ROLE_RECEPTIONIST],
                        |db, id| Box::pin(services::confirm_appointment(db, id)),
                    )
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
                    run_transition(&db, &params, req, &[ROLE_DOCTOR], |db, id| {
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
                    run_transition(&db, &params, req, &[ROLE_DOCTOR], |db, id| {
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
                    run_transition(
                        &db,
                        &params,
                        req,
                        &[ROLE_RECEPTIONIST, ROLE_DOCTOR],
                        |db, id| Box::pin(services::cancel_appointment(db, id)),
                    )
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

async fn login(db: &Db, req: Request) -> Result<Response, Error> {
    let body: LoginRequest = read_json(req).await?;

    // Constant-time-ish lookup: verify against a dummy hash when the
    // user row is missing, so the response latency doesn't leak
    // whether the email exists. `password::verify` is itself
    // constant-time.
    let user_row = user::find_by_email(db, &body.email).await?;
    let valid = match &user_row {
        Some(u) => u.is_active && password::verify(&body.password, &u.password_hash),
        None => {
            let _ = password::verify(&body.password, rustio_core::auth::dummy_password_hash());
            false
        }
    };
    let actor = match (valid, user_row) {
        (true, Some(u)) => u,
        _ => return Err(Error::Unauthorized),
    };

    let sess = session::create(db, actor.id).await?;
    Ok(json_response(
        200,
        &LoginResponse {
            token: sess.id,
            role: actor.role,
            user_id: actor.id,
            expires_at: sess.expires_at,
        },
    ))
}

async fn schedule(db: &Db, req: Request) -> Result<Response, Error> {
    let actor = require_actor(db, &req).await?;
    require_role(&actor, &[ROLE_RECEPTIONIST])?;
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
/// so one helper handles them all. `allowed_roles` is the per-route
/// permission set; the `admin` role always passes via
/// [`require_role`].
async fn run_transition<'a, F>(
    db: &'a Db,
    params: &'a Params,
    req: Request,
    allowed_roles: &[&str],
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
    let actor = require_actor(db, &req).await?;
    require_role(&actor, allowed_roles)?;
    let id = parse_id(params)?;
    service_fn(db, id).await?;
    Ok(json_response(200, &OkResponse { ok: true }))
}

async fn check_in(db: &Db, params: &Params, req: Request) -> Result<Response, Error> {
    let actor = require_actor(db, &req).await?;
    require_role(&actor, &[ROLE_RECEPTIONIST])?;
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
    Ok(json_response(201, &IdResponse { id: check_in_id }))
}

// ═══════════════════════════════════════════════════════════════
// Request / Response DTOs
// ═══════════════════════════════════════════════════════════════

#[derive(Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Serialize)]
struct LoginResponse {
    /// Opaque bearer token — pass back as `Authorization: Bearer <token>`.
    token: String,
    role: String,
    user_id: i64,
    expires_at: DateTime<Utc>,
}

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
// Authentication + role enforcement
// ═══════════════════════════════════════════════════════════════

/// Resolve the current actor from the request's `Authorization`
/// header. Returns the full [`User`] on success — the caller is
/// expected to follow up with [`require_role`] for per-route
/// permission checks.
///
///   * Missing / malformed / expired token → [`Error::Unauthorized`] (401)
///   * Token valid but the backing user is deactivated → [`Error::Forbidden`] (403)
async fn require_actor(db: &Db, req: &Request) -> Result<User, Error> {
    let token = extract_bearer(req)?;
    let sess = session::find_valid(db, &token)
        .await?
        .ok_or(Error::Unauthorized)?;
    let actor = user::find_by_id(db, sess.user_id)
        .await?
        .ok_or(Error::Unauthorized)?;
    if !actor.is_active {
        return Err(Error::Forbidden);
    }
    Ok(actor)
}

/// Reject unless the actor's role is in `allowed` (or the actor is
/// an `admin`, which bypasses every role check as a super-role).
fn require_role(actor: &User, allowed: &[&str]) -> Result<(), Error> {
    if actor.role == ROLE_ADMIN {
        return Ok(());
    }
    if allowed.contains(&actor.role.as_str()) {
        Ok(())
    } else {
        Err(Error::Forbidden)
    }
}

/// Pull the raw token out of `Authorization: Bearer <token>`. Every
/// failure mode collapses to [`Error::Unauthorized`] so we don't
/// leak whether a header was present / non-UTF-8 / wrong scheme.
fn extract_bearer(req: &Request) -> Result<String, Error> {
    let header = req
        .headers()
        .get(hyper::header::AUTHORIZATION)
        .ok_or(Error::Unauthorized)?;
    let raw = header.to_str().map_err(|_| Error::Unauthorized)?;
    raw.strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .ok_or(Error::Unauthorized)
}
