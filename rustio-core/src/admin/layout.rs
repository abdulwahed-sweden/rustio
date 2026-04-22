pub fn render_layout(content: String, sidebar: String) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>Admin</title>

<style>
:root {{
  --bg-page: #F4F6F9;
  --bg-surface: #FFFFFF;
  --ink: #141A21;
  --border: #D8DDE5;
  --accent: #B84318;
}}

body {{
  margin: 0;
  font-family: Inter, system-ui, sans-serif;
  background: var(--bg-page);
  color: var(--ink);
}}

.topbar {{
  height: 56px;
  background: #141A21;
  color: white;
  display: flex;
  align-items: center;
  padding: 0 20px;
}}

.layout {{
  display: grid;
  grid-template-columns: 240px 1fr;
}}

.sidebar {{
  background: var(--bg-surface);
  border-right: 1px solid var(--border);
  min-height: calc(100vh - 56px);
  padding: 16px;
}}

.main {{
  padding: 24px;
}}
</style>

</head>
<body>

<div class="topbar">
  <strong>RustIO Admin</strong>
</div>

<div class="layout">
  <aside class="sidebar">
    {sidebar}
  </aside>

  <main class="main">
    {content}
  </main>
</div>

</body>
</html>"#
    )
}

pub fn render_sidebar() -> String {
    r#"
    <div>Users</div>
    <div>Appointments</div>
    <div>Invoices</div>
    "#
    .to_string()
}

pub fn admin_index() -> String {
    let content = "<h1>Admin Ready</h1>".to_string();
    let sidebar = render_sidebar();

    render_layout(content, sidebar)
}
