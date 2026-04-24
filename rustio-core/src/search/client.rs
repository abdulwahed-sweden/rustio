//! A minimal Meilisearch REST client. We don't pull in
//! `meilisearch-sdk` because we use a small subset — index CRUD, add
//! documents, search — and the sdk adds several hundred KB of
//! dependencies for features we don't need.

use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

#[derive(Clone)]
pub struct MeiliClient {
    http: Client,
    base_url: String,
    api_key: Option<String>,
}

impl MeiliClient {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(2))
            .pool_idle_timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(8)
            .build()
            .map_err(|e| Error::Internal(format!("reqwest build: {e}")))?;
        Ok(Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key,
        })
    }

    pub async fn health(&self) -> Result<()> {
        let url = format!("{}/health", self.base_url);
        let resp = self.http.get(&url).send().await.map_err(req_err)?;
        if !resp.status().is_success() {
            return Err(Error::Internal(format!(
                "meili health returned {}",
                resp.status()
            )));
        }
        Ok(())
    }

    /// Push documents into an index. Meili creates the index on the
    /// first call — no explicit `create_index` required.
    pub async fn add_documents(
        &self,
        index: &str,
        documents: &[Value],
        primary_key: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/indexes/{}/documents?primaryKey={}",
            self.base_url, index, primary_key
        );
        let mut req = self.http.post(&url).json(documents);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req.send().await.map_err(req_err)?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Internal(format!(
                "meili add_documents {status}: {body}"
            )));
        }
        Ok(())
    }

    pub async fn delete_document(&self, index: &str, id: &str) -> Result<()> {
        let url = format!("{}/indexes/{}/documents/{}", self.base_url, index, id);
        let mut req = self.http.delete(&url);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req.send().await.map_err(req_err)?;
        if !resp.status().is_success() && resp.status() != 404 {
            return Err(Error::Internal(format!(
                "meili delete_document: {}",
                resp.status()
            )));
        }
        Ok(())
    }

    pub async fn search(
        &self,
        index: &str,
        query: &str,
        options: &SearchOptions,
    ) -> Result<SearchResults> {
        let url = format!("{}/indexes/{}/search", self.base_url, index);
        let body = SearchRequest {
            q: query,
            limit: options.limit,
            offset: options.offset,
            filter: options.filter.as_deref(),
            sort: options.sort.as_deref(),
            attributes_to_highlight: options.highlight.as_deref(),
            facets: options.facets.as_deref(),
            highlight_pre_tag: options.highlight_pre_tag.as_deref(),
            highlight_post_tag: options.highlight_post_tag.as_deref(),
        };
        let mut req = self.http.post(&url).json(&body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req.send().await.map_err(req_err)?;
        if !resp.status().is_success() {
            return Err(Error::Internal(format!("meili search: {}", resp.status())));
        }
        let parsed: SearchResults = resp.json().await.map_err(req_err)?;
        Ok(parsed)
    }

    pub async fn configure_index(
        &self,
        index: &str,
        searchable: &[&str],
        filterable: &[&str],
        sortable: &[&str],
    ) -> Result<()> {
        let url = format!("{}/indexes/{}/settings", self.base_url, index);
        let body = serde_json::json!({
            "searchableAttributes": searchable,
            "filterableAttributes": filterable,
            "sortableAttributes": sortable,
        });
        let mut req = self.http.patch(&url).json(&body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req.send().await.map_err(req_err)?;
        if !resp.status().is_success() {
            return Err(Error::Internal(format!(
                "meili configure_index: {}",
                resp.status()
            )));
        }
        Ok(())
    }
}

fn req_err(e: reqwest::Error) -> Error {
    Error::Internal(format!("reqwest: {e}"))
}

#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub filter: Option<String>,
    pub sort: Option<Vec<String>>,
    pub highlight: Option<Vec<String>>,
    pub facets: Option<Vec<String>>,
    /// Markers wrapped around every matched substring in the `_formatted`
    /// payload. Keep them distinctive so the renderer can swap them out
    /// for `<em>` / `</em>` (or anything else) safely.
    pub highlight_pre_tag: Option<String>,
    pub highlight_post_tag: Option<String>,
}

#[derive(Serialize)]
struct SearchRequest<'a> {
    q: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    filter: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "sort")]
    sort: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "attributesToHighlight")]
    attributes_to_highlight: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    facets: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "highlightPreTag")]
    highlight_pre_tag: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "highlightPostTag")]
    highlight_post_tag: Option<&'a str>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SearchResults {
    #[serde(default)]
    pub hits: Vec<SearchHit>,
    #[serde(default, rename = "estimatedTotalHits")]
    pub estimated_total: u64,
    #[serde(default, rename = "processingTimeMs")]
    pub processing_time_ms: u64,
    /// For every requested facet, `{value → count}`. Empty when the
    /// request did not ask for facets.
    #[serde(
        default,
        rename = "facetDistribution",
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    pub facet_distribution:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, u64>>,
}

pub type SearchHit = Value;
