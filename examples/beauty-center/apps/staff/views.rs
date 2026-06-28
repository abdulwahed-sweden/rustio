use rustio_core::{html, Error, Response, Router};

/// Tutorial page for the `Staff` app. `GET /staff` confirms the app is wired up;
/// the CRUD admin is generated at `/admin/staff`.
pub fn register(router: Router) -> Router {
    router.get("/staff", |_req, _params| async {
        Ok::<Response, Error>(html(PAGE))
    })
}

const PAGE: &str = r#"<!doctype html><html lang="en"><head><meta charset="utf-8">
<title>Staff — Beauty Center</title></head>
<body style="font-family:system-ui,-apple-system,sans-serif;background:#f5f7fb;color:#16223c;display:grid;place-items:center;min-height:100vh;margin:0">
<main style="background:#fff;border:1px solid #e5e9f2;border-radius:12px;padding:2rem 2.5rem;max-width:30rem">
<h1 style="margin:0 0 .25rem">It works.</h1>
<p style="color:#697892;margin:0 0 1.25rem">Staff app · Beauty Center — serves <code>/staff</code>. The CRUD admin is generated.</p>
<a href="/admin/staffs" style="display:inline-block;background:#2B54E0;color:#fff;padding:.6rem 1.1rem;border-radius:8px;text-decoration:none;font-weight:600">Open admin →</a>
</main></body></html>"#;
