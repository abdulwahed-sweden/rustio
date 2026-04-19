use rustio_core::{html, Error, Response, Router};

/// Tutorial page for the `Post` app.
///
/// Hitting `GET /posts` returns the HTML below so you can confirm the
/// app is wired up. Replace this handler with your real view — this file
/// is yours to edit freely.
pub fn register(router: Router) -> Router {
    router.get("/posts", |_req, _params| async {
        Ok::<Response, Error>(html(WELCOME_HTML))
    })
}

const WELCOME_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Post — RustIO</title>
<style>
  *, *::before, *::after { box-sizing: border-box; }
  html, body { height: 100%; margin: 0; }
  body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
         background: #fafafa; color: #222; display: flex; align-items: center; justify-content: center; }
  main { max-width: 32rem; padding: 2.5rem; background: white; border-radius: 8px;
         box-shadow: 0 4px 20px rgba(0,0,0,0.05); text-align: left; }
  h1 { margin: 0 0 0.25rem; font-size: 1.5rem; }
  .tag { color: #888; font-size: 0.9rem; margin: 0 0 1.5rem; }
  p { line-height: 1.55; margin: 0.75rem 0; }
  code { background: #f0f0f2; padding: 0.1rem 0.35rem; border-radius: 3px; font-size: 0.9em; }
  a { color: #0366d6; }
  .actions { margin-top: 1.5rem; display: flex; gap: 0.5rem; flex-wrap: wrap; }
  .btn { padding: 0.55rem 1rem; border-radius: 5px; text-decoration: none; font-size: 0.95rem; font-weight: 500; }
  .btn.primary { background: #222; color: white; }
  .btn.secondary { background: #f0f0f2; color: #222; }
</style>
</head>
<body>
<main>
  <h1>It works.</h1>
  <p class="tag">Post app · RustIO</p>
  <p>Your <code>Post</code> app is wired up and serving this page at <code>/posts</code>.</p>
  <p>To build a real view, edit <code>apps/posts/views.rs</code>. The CRUD admin for this model is already generated and ready to use.</p>
  <div class="actions">
    <a class="btn primary" href="/admin/posts">Open admin</a>
    <a class="btn secondary" href="/">Home</a>
  </div>
</main>
</body>
</html>"##;
