//! /search — parse query params, call Meilisearch, return a shape the
//! JSON endpoint and the HTML template both consume.
//!
//! Params:
//! - q           full-text query (default "")
//! - published   "true" | "false" | missing (missing = no filter)
//! - author      comma-separated author names (OR within the group)
//! - date_range  "week" | "month" | "year" | missing
//! - sort        "relevance" (default) | "newest" | "oldest"
//! - page        1-based, default 1. Page size = 20.
//! - format      "json" (default renders JSON from this handler; the
//!   HTML route wraps the same shape into a template).

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{Duration, Utc};
use serde::Serialize;

use rustio_core::error::Result;
use rustio_core::http::{FormData, Request, Response};
use rustio_core::orm::Db;
use rustio_core::search::{MeiliClient, Searchable, SearchOptions};

pub const PAGE_SIZE: u64 = 20;
pub const HIGHLIGHT_PRE: &str = "<mark>";
pub const HIGHLIGHT_POST: &str = "</mark>";

/// Everything a search response needs, in one struct. Serialisable to
/// JSON directly; the HTML route feeds it to minijinja.
#[derive(Debug, Serialize)]
pub struct SearchView {
    pub q: String,
    pub hits: Vec<serde_json::Value>,
    pub total: u64,
    pub ms: u64,
    pub page: u64,
    pub page_size: u64,
    pub total_pages: u64,
    pub sort: String,
    pub filters: ActiveFilters,
    pub facets: BTreeMap<String, BTreeMap<String, u64>>,
}

#[derive(Debug, Serialize, Default)]
pub struct ActiveFilters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub author: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_range: Option<String>,
}

/// Parse query params + run the Meili search. Shared by the HTML and
/// JSON routes so a change to param handling only lives in one place.
pub async fn run_search(meili: &MeiliClient, query: &FormData) -> Result<SearchView> {
    let q = query.get("q").unwrap_or("").to_string();
    let filters = parse_filters(query);
    let sort = parse_sort(query);
    let page = parse_page(query);

    let filter_expr = build_filter(&filters);
    let sort_list = sort_clause(&sort);

    let facets: Vec<String> = crate::apps::posts::Post::FACETABLE_ATTRIBUTES
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let highlight: Vec<String> = ["title", "body"].iter().map(|s| (*s).to_string()).collect();

    let opts = SearchOptions {
        limit: Some(PAGE_SIZE),
        offset: Some((page.saturating_sub(1)) * PAGE_SIZE),
        filter: filter_expr,
        sort: sort_list,
        highlight: Some(highlight),
        facets: Some(facets),
        highlight_pre_tag: Some(HIGHLIGHT_PRE.into()),
        highlight_post_tag: Some(HIGHLIGHT_POST.into()),
    };

    let results = meili.search("posts", &q, &opts).await?;
    let total_pages = results.estimated_total.div_ceil(PAGE_SIZE).max(1);
    let now = Utc::now().timestamp();
    let hits: Vec<serde_json::Value> = results
        .hits
        .into_iter()
        .map(|mut h| {
            if let Some(ts) = h.get("created_at").and_then(|v| v.as_i64()) {
                if let Some(obj) = h.as_object_mut() {
                    obj.insert(
                        "created_at_display".into(),
                        serde_json::Value::String(relative_time(now, ts)),
                    );
                }
            }
            h
        })
        .collect();

    Ok(SearchView {
        q,
        hits,
        total: results.estimated_total,
        ms: results.processing_time_ms,
        page,
        page_size: PAGE_SIZE,
        total_pages,
        sort,
        filters,
        facets: results.facet_distribution,
    })
}

/// Route handler: GET /search?...&format=json — always JSON.
pub async fn search_json(
    _db: &Db,
    meili: &Arc<MeiliClient>,
    req: Request,
) -> Result<Response> {
    let view = run_search(meili, &req.query()).await?;
    let body = serde_json::to_string(&view)?;
    Ok(Response::json_raw(body))
}

/// Route handler: GET /search — renders the HTML page with results
/// baked in for the first paint. The same JSON shape is serialised
/// into a `data-initial` attribute so the client-side script can
/// hydrate without a second fetch.
pub async fn search_html(
    meili: &Arc<MeiliClient>,
    templates: &std::sync::Arc<rustio_core::templates::Templates>,
    req: Request,
) -> Result<Response> {
    let view = run_search(meili, &req.query()).await?;
    // Pass as a raw JSON string; minijinja auto-escapes it into the
    // `data-initial` attribute. Double-escaping would break the JS parse.
    let initial_json = serde_json::to_string(&view)?;

    let ctx = serde_json::json!({
        "view": view,
        "initial_json": initial_json,
        // Anonymous page — no identity. The template guards the block.
        "identity": serde_json::Value::Null,
    });
    let body = templates.render("search.html", &ctx)?;
    Ok(Response::html(body))
}

// --- helpers ---------------------------------------------------------------

fn parse_filters(q: &FormData) -> ActiveFilters {
    let published = match q.get("published") {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    };
    let author: Vec<String> = q
        .get("author")
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();
    let date_range = match q.get("date_range") {
        Some(v @ ("week" | "month" | "year")) => Some(v.to_string()),
        _ => None,
    };
    ActiveFilters { published, author, date_range }
}

fn parse_sort(q: &FormData) -> String {
    match q.get("sort") {
        Some(v @ ("newest" | "oldest")) => v.to_string(),
        _ => "relevance".to_string(),
    }
}

fn parse_page(q: &FormData) -> u64 {
    q.get("page")
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(1)
}

/// Meili filter syntax: `a AND b AND c`. Each value needs quoting
/// because author names and booleans coexist in one expression.
fn build_filter(f: &ActiveFilters) -> Option<String> {
    let mut clauses: Vec<String> = Vec::new();
    if let Some(b) = f.published {
        clauses.push(format!("published = {b}"));
    }
    if !f.author.is_empty() {
        let quoted: Vec<String> = f
            .author
            .iter()
            .map(|a| format!("author = \"{}\"", escape_quotes(a)))
            .collect();
        clauses.push(format!("({})", quoted.join(" OR ")));
    }
    if let Some(range) = &f.date_range {
        let delta = match range.as_str() {
            "week" => Duration::days(7),
            "month" => Duration::days(30),
            "year" => Duration::days(365),
            _ => return Some(clauses.join(" AND ")),
        };
        let since = (Utc::now() - delta).timestamp();
        clauses.push(format!("created_at >= {since}"));
    }
    if clauses.is_empty() {
        None
    } else {
        Some(clauses.join(" AND "))
    }
}

fn sort_clause(sort: &str) -> Option<Vec<String>> {
    match sort {
        "newest" => Some(vec!["created_at:desc".into()]),
        "oldest" => Some(vec!["created_at:asc".into()]),
        _ => None, // "relevance" = let Meili rank
    }
}

fn escape_quotes(s: &str) -> String {
    s.replace('"', "\\\"")
}

/// "2 days ago" / "just now" / "3 months ago". Both sides of the network
/// format the same way; see `search.js:relativeTime`.
fn relative_time(now_ts: i64, then_ts: i64) -> String {
    let delta = now_ts.saturating_sub(then_ts);
    if delta < 60 {
        return "just now".into();
    }
    let (n, unit) = if delta < 3600 {
        (delta / 60, "minute")
    } else if delta < 86_400 {
        (delta / 3600, "hour")
    } else if delta < 604_800 {
        (delta / 86_400, "day")
    } else if delta < 2_629_800 {
        (delta / 604_800, "week")
    } else if delta < 31_557_600 {
        (delta / 2_629_800, "month")
    } else {
        (delta / 31_557_600, "year")
    };
    let plural = if n == 1 { "" } else { "s" };
    format!("{n} {unit}{plural} ago")
}
