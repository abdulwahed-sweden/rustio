use rustio_core::{html, Error, Response, Router};

/// Tutorial page for the `Customer` app. `GET /customers` confirms the app is
/// wired up; replace this handler with your real view.
pub fn register(router: Router) -> Router {
    router.get("/customers", |_req, _params| async {
        Ok::<Response, Error>(html(WELCOME_HTML))
    })
}

const WELCOME_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Customer — bookflow</title>
<style>
  body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
         background: #fafafa; color: #222; display: flex; min-height: 100vh; margin: 0;
         align-items: center; justify-content: center; }
  main { max-width: 32rem; padding: 2.5rem; background: #fff; border-radius: 8px;
         box-shadow: 0 4px 20px rgba(0,0,0,0.05); }
  h1 { margin: 0 0 .25rem; font-size: 1.5rem; }
  .tag { color: #888; font-size: .9rem; margin: 0 0 1.5rem; }
  code { background: #f0f0f2; padding: .1rem .35rem; border-radius: 3px; }
  a.btn { display: inline-block; margin-top: 1.25rem; padding: .55rem 1rem; border-radius: 5px;
          background: #222; color: #fff; text-decoration: none; font-weight: 500; }
</style>
</head>
<body>
<main>
  <h1>It works.</h1>
  <p class="tag">Customer app · bookflow</p>
  <p>Your <code>Customer</code> app serves this page at <code>/customers</code>. Edit
     <code>apps/customers/views.rs</code> to build a real view; the CRUD admin is already generated.</p>
  <a class="btn" href="/admin/customers">Open admin</a>
</main>
</body>
</html>"##;
