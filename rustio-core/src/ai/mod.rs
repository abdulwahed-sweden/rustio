//! The AI layer. Three deterministic stages, all working from the same
//! fixed `Primitive` vocabulary:
//!
//! - `plan`   — parses natural language into a typed plan
//! - `review` — scores risk + impact against the current schema
//! - `apply`  — writes files (models.rs + migration)
//!
//! The hard rule: if a change cannot be expressed as a `Primitive`, it
//! is **rejected**. No free-form code generation, no "close enough"
//! fallback.

mod executor;
mod planner;
mod primitive;
mod review;

pub use executor::{apply_plan, ApplyOptions, ApplyOutcome, ApplyError};
pub use planner::{plan, PlanError};
pub use primitive::{FieldSpec, Plan, Primitive};
pub use review::{review, Impact, Risk, Review};
