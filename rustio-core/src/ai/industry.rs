//! Industry schemas — 0.6.0.
//!
//! A small, deliberately-closed registry of industry-specific
//! conventions. Each entry answers two questions:
//!
//! 1. Which field names does a project in this industry *typically*
//!    need to model? (`required_fields`)
//! 2. What non-obvious rules of thumb should a reviewer see?
//!    (`conventions`)
//!
//! The registry is **informational**, not enforcement. The executor
//! uses it only to raise warnings when a plan removes a required
//! field; it never auto-adds fields or blocks execution purely on a
//! missing convention. Enforcement lives in the context layer proper
//! (`ContextConfig::pii_fields` + the executor's policy gate).
//!
//! ## Why rule-based + hard-coded
//!
//! This is a *safety surface*. A loose definition of "what banking
//! needs" would produce unreliable warnings. Hard-coding a small set
//! of industries — housing, healthcare, banking — keeps every entry
//! auditable in one file. New industries land through explicit PRs,
//! not via config files.
//!
//! ## Extending
//!
//! Add a variant to `industry_schema_for`. Keep the list terse and
//! universal: if a rule is obvious from the field name, don't list
//! it. Projects with idiosyncratic conventions should own them in
//! their own `rustio.context.json`, not in this registry.

/// The 0.6.0 shape of an industry convention. Kept intentionally tiny;
/// richer representations (per-field type hints, regex patterns, …)
/// land in later passes as their need is proven.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndustrySchema {
    /// Field names a reviewer would expect on at least one model in
    /// this industry. The review layer warns when a plan proposes
    /// removing any of these.
    pub required_fields: Vec<String>,
    /// Human-readable sentences the CLI surfaces under
    /// `rustio context show`. Never parsed, never matched against —
    /// purely for operators reading the output.
    pub conventions: Vec<String>,
}

/// Look up the convention bundle for a named industry. Case-insensitive.
/// Returns `None` for industries the registry doesn't know — the caller
/// then falls back to generic rules.
pub fn industry_schema_for(industry: &str) -> Option<IndustrySchema> {
    match industry.to_lowercase().as_str() {
        "housing" => Some(IndustrySchema {
            required_fields: vec![
                "personnummer".into(),
                "queue_start_date".into(),
                "annual_income".into(),
            ],
            conventions: vec![
                "Housing models track a personal identifier (personnummer in SE), a queue-start date, and an income declaration.".into(),
                "Listings usually carry monthly_rent (in local currency minor units) and a boolean `is_active` for visibility.".into(),
            ],
        }),
        "healthcare" => Some(IndustrySchema {
            required_fields: vec!["patient_id".into(), "created_at".into()],
            conventions: vec![
                "Patient identifiers should be opaque strings (UUID / hashed). Sequential integers leak enrolment order and are refused by the planner under this industry.".into(),
                "Health records typically carry created_at, updated_at, and a soft-delete via deleted_at rather than physical removal.".into(),
            ],
        }),
        "banking" => Some(IndustrySchema {
            required_fields: vec![
                "account_number".into(),
                "currency".into(),
                "balance".into(),
            ],
            conventions: vec![
                "Monetary values are stored as integer minor units (öre, cents). Floating-point types are refused by the planner under this industry.".into(),
                "Account numbers are stored as String; i32 has insufficient range for international account number formats.".into(),
            ],
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn housing_has_personnummer_as_a_required_field() {
        let h = industry_schema_for("housing").unwrap();
        assert!(h.required_fields.iter().any(|f| f == "personnummer"));
    }

    #[test]
    fn case_insensitive_lookup() {
        assert!(industry_schema_for("HEALTHCARE").is_some());
        assert!(industry_schema_for("Healthcare").is_some());
    }

    #[test]
    fn unknown_industry_returns_none() {
        assert!(industry_schema_for("martian_postal_service").is_none());
    }
}
