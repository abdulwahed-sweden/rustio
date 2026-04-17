use std::collections::HashMap;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};

use crate::auth::require_admin;
use crate::error::Error;
use crate::http::{Request, Response, html};
use crate::orm::{Db, Model};
use crate::router::Router;

#[derive(Debug, Clone, Copy)]
pub enum FieldType {
    I32,
    I64,
    String,
    Bool,
}

#[derive(Debug, Clone, Copy)]
pub struct AdminField {
    pub name: &'static str,
    pub ty: FieldType,
    pub editable: bool,
}

pub trait AdminModel: Model {
    const ADMIN_NAME: &'static str;
    const DISPLAY_NAME: &'static str;
    const FIELDS: &'static [AdminField];

    fn field_display(&self, name: &str) -> Option<String>;
    fn from_form(form: &FormData, id: Option<i64>) -> Result<Self, Error>;
}

pub struct FormData {
    map: HashMap<String, String>,
}

impl FormData {
    pub fn parse(body: &str) -> Self {
        let mut map = HashMap::new();
        for pair in body.split('&') {
            if pair.is_empty() {
                continue;
            }
            let mut iter = pair.splitn(2, '=');
            let raw_key = match iter.next() {
                Some(k) if !k.is_empty() => k,
                _ => continue,
            };
            let raw_val = iter.next().unwrap_or("");
            map.insert(percent_decode(raw_key), percent_decode(raw_val));
        }
        FormData { map }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

pub fn register<T>(mut router: Router, db: &Db) -> Router
where
    T: AdminModel + Model,
{
    let base = format!("/admin/{}", T::ADMIN_NAME);
    let create_path = format!("{base}/create");
    let edit_path = format!("{base}/:id/edit");
    let delete_path = format!("{base}/:id/delete");

    let list_db = db.clone();
    router = router.get(&base, move |req, _params| {
        let db = list_db.clone();
        async move {
            require_admin(req.ctx())?;
            let items = T::all(&db).await?;
            Ok::<Response, Error>(html(admin_layout(
                T::DISPLAY_NAME,
                &list_page::<T>(&items),
            )))
        }
    });

    router = router.get(&create_path, |req, _params| async move {
        require_admin(req.ctx())?;
        Ok::<Response, Error>(html(admin_layout(
            &format!("New {}", T::DISPLAY_NAME),
            &form_page::<T>(None, &format!("/admin/{}/create", T::ADMIN_NAME)),
        )))
    });

    let create_db = db.clone();
    router = router.post(&create_path, move |req, _params| {
        let db = create_db.clone();
        async move {
            require_admin(req.ctx())?;
            let form = read_form(req).await?;
            let item = T::from_form(&form, None)?;
            item.create(&db).await?;
            Ok::<Response, Error>(redirect(&format!("/admin/{}", T::ADMIN_NAME)))
        }
    });

    let edit_db = db.clone();
    router = router.get(&edit_path, move |req, params| {
        let db = edit_db.clone();
        async move {
            require_admin(req.ctx())?;
            let id = parse_id_param(&params)?;
            let item = T::find(&db, id).await?.ok_or(Error::NotFound)?;
            Ok::<Response, Error>(html(admin_layout(
                &format!("Edit {}", T::DISPLAY_NAME),
                &form_page::<T>(
                    Some(&item),
                    &format!("/admin/{}/{}/edit", T::ADMIN_NAME, id),
                ),
            )))
        }
    });

    let update_db = db.clone();
    router = router.post(&edit_path, move |req, params| {
        let db = update_db.clone();
        async move {
            require_admin(req.ctx())?;
            let id = parse_id_param(&params)?;
            let form = read_form(req).await?;
            let item = T::from_form(&form, Some(id))?;
            item.update(&db).await?;
            Ok::<Response, Error>(redirect(&format!("/admin/{}", T::ADMIN_NAME)))
        }
    });

    let delete_db = db.clone();
    router = router.post(&delete_path, move |req, params| {
        let db = delete_db.clone();
        async move {
            require_admin(req.ctx())?;
            let id = parse_id_param(&params)?;
            T::delete(&db, id).await?;
            Ok::<Response, Error>(redirect(&format!("/admin/{}", T::ADMIN_NAME)))
        }
    });

    router
}

fn parse_id_param(params: &crate::router::Params) -> Result<i64, Error> {
    params
        .get("id")
        .and_then(|s| s.parse::<i64>().ok())
        .ok_or_else(|| Error::BadRequest(String::from("invalid id")))
}

async fn read_form(req: Request) -> Result<FormData, Error> {
    let (_, body, _) = req.into_parts();
    let collected = body
        .collect()
        .await
        .map_err(|e| Error::BadRequest(e.to_string()))?
        .to_bytes();
    let body_str = std::str::from_utf8(&collected)
        .map_err(|e| Error::BadRequest(e.to_string()))?;
    Ok(FormData::parse(body_str))
}

fn redirect(to: &str) -> Response {
    hyper::Response::builder()
        .status(303)
        .header("location", to)
        .body(Full::new(Bytes::new()))
        .expect("valid redirect")
}

fn admin_layout(title: &str, content: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} — RustIO Admin</title>
<style>{css}</style>
</head>
<body>
<header><h1>RustIO Admin</h1></header>
<main>{content}</main>
</body>
</html>"#,
        title = escape_html(title),
        css = ADMIN_CSS,
        content = content,
    )
}

fn list_page<T: AdminModel>(items: &[T]) -> String {
    let headers: String = T::FIELDS
        .iter()
        .map(|f| format!("<th>{}</th>", escape_html(f.name)))
        .collect();
    let rows: String = items
        .iter()
        .map(|item| {
            let cells: String = T::FIELDS
                .iter()
                .map(|f| {
                    let v = item.field_display(f.name).unwrap_or_default();
                    format!("<td>{}</td>", escape_html(&v))
                })
                .collect();
            let id = item.id();
            let actions = format!(
                r#"<td class="actions">
<a href="/admin/{name}/{id}/edit">edit</a>
<form method="post" action="/admin/{name}/{id}/delete">
<button type="submit" class="danger">delete</button>
</form>
</td>"#,
                name = T::ADMIN_NAME,
                id = id,
            );
            format!("<tr>{cells}{actions}</tr>")
        })
        .collect();

    format!(
        r#"<div class="toolbar">
<h2>{title}</h2>
<a class="button" href="/admin/{name}/create">New {display}</a>
</div>
<table>
<thead><tr>{headers}<th>actions</th></tr></thead>
<tbody>{rows}</tbody>
</table>"#,
        title = escape_html(T::DISPLAY_NAME),
        display = escape_html(T::DISPLAY_NAME),
        name = T::ADMIN_NAME,
    )
}

fn form_page<T: AdminModel>(item: Option<&T>, action: &str) -> String {
    let fields: String = T::FIELDS
        .iter()
        .filter(|f| f.editable)
        .map(|f| render_field::<T>(f, item))
        .collect();
    let heading = if item.is_some() {
        format!("Edit {}", T::DISPLAY_NAME)
    } else {
        format!("New {}", T::DISPLAY_NAME)
    };
    format!(
        r#"<h2>{heading}</h2>
<form method="post" action="{action}">
{fields}
<div class="form-actions">
<button type="submit">Save</button>
<a class="cancel" href="/admin/{name}">Cancel</a>
</div>
</form>"#,
        heading = escape_html(&heading),
        action = escape_html(action),
        name = T::ADMIN_NAME,
    )
}

fn render_field<T: AdminModel>(f: &AdminField, item: Option<&T>) -> String {
    let current = item.and_then(|i| i.field_display(f.name)).unwrap_or_default();
    let input = match f.ty {
        FieldType::Bool => format!(
            r#"<input type="checkbox" name="{n}" {checked}>"#,
            n = escape_html(f.name),
            checked = if current == "true" { "checked" } else { "" },
        ),
        FieldType::I32 | FieldType::I64 => format!(
            r#"<input type="number" name="{n}" value="{v}">"#,
            n = escape_html(f.name),
            v = escape_html(&current),
        ),
        FieldType::String => format!(
            r#"<input type="text" name="{n}" value="{v}">"#,
            n = escape_html(f.name),
            v = escape_html(&current),
        ),
    };
    format!(
        r#"<label><span>{label}</span>{input}</label>"#,
        label = escape_html(f.name),
        input = input,
    )
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'+' {
            out.push(b' ');
            i += 1;
        } else if b == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
            out.push(b);
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

const ADMIN_CSS: &str = r#"
*, *::before, *::after { box-sizing: border-box; }
body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  background: #fafafa; color: #222; margin: 0; }
header { background: #222; color: white; padding: 1rem 2rem; }
header h1 { margin: 0; font-size: 1.1rem; font-weight: 600; letter-spacing: 0.02em; }
main { padding: 2rem; max-width: 60rem; margin: 0 auto; }
h2 { margin: 0; }
.toolbar { display: flex; align-items: center; justify-content: space-between; margin-bottom: 1.5rem; }
table { border-collapse: collapse; width: 100%; background: white; border-radius: 6px; overflow: hidden;
  box-shadow: 0 1px 3px rgba(0,0,0,0.04); }
th, td { text-align: left; padding: 0.6rem 0.9rem; border-bottom: 1px solid #eee; font-size: 0.95rem; }
th { background: #f4f4f5; font-weight: 600; }
tbody tr:last-child td { border-bottom: none; }
td.actions { display: flex; gap: 0.5rem; align-items: center; }
td.actions form { margin: 0; display: inline; }
a { color: #0366d6; text-decoration: none; }
a:hover { text-decoration: underline; }
label { display: block; margin-bottom: 1rem; }
label span { display: block; font-weight: 500; margin-bottom: 0.25rem; font-size: 0.9rem; }
input[type=text], input[type=number] { padding: 0.5rem 0.75rem; border: 1px solid #d0d0d4;
  border-radius: 4px; width: 24rem; max-width: 100%; font: inherit; }
input[type=checkbox] { transform: scale(1.1); }
button, .button { padding: 0.5rem 1rem; background: #222; color: white; border: none;
  border-radius: 4px; cursor: pointer; font: inherit; text-decoration: none; display: inline-block; }
button:hover, .button:hover { background: #000; text-decoration: none; }
button.danger { background: #b42318; }
button.danger:hover { background: #8a1c12; }
.form-actions { display: flex; gap: 0.5rem; align-items: center; margin-top: 1rem; }
.form-actions .cancel { color: #666; }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_parse_decodes_basic_pairs() {
        let form = FormData::parse("a=1&b=2");
        assert_eq!(form.get("a"), Some("1"));
        assert_eq!(form.get("b"), Some("2"));
    }

    #[test]
    fn form_parse_decodes_plus_as_space() {
        let form = FormData::parse("name=John+Doe");
        assert_eq!(form.get("name"), Some("John Doe"));
    }

    #[test]
    fn form_parse_decodes_percent_encoded() {
        let form = FormData::parse("q=hello%20world%21");
        assert_eq!(form.get("q"), Some("hello world!"));
    }

    #[test]
    fn form_parse_handles_empty_values() {
        let form = FormData::parse("a=&b=x");
        assert_eq!(form.get("a"), Some(""));
        assert_eq!(form.get("b"), Some("x"));
    }

    #[test]
    fn form_parse_ignores_empty_pairs() {
        let form = FormData::parse("&a=1&&b=2&");
        assert_eq!(form.get("a"), Some("1"));
        assert_eq!(form.get("b"), Some("2"));
        assert_eq!(form.len(), 2);
    }

    #[test]
    fn form_missing_key_is_none() {
        let form = FormData::parse("a=1");
        assert!(form.get("missing").is_none());
    }

    #[test]
    fn escape_html_escapes_dangerous_chars() {
        assert_eq!(
            escape_html("<script>alert(\"xss\")</script>"),
            "&lt;script&gt;alert(&quot;xss&quot;)&lt;/script&gt;"
        );
        assert_eq!(escape_html("a & b"), "a &amp; b");
        assert_eq!(escape_html("it's"), "it&#39;s");
    }

    #[test]
    fn percent_decode_passes_through_unreserved() {
        assert_eq!(percent_decode("abcXYZ123-_.~"), "abcXYZ123-_.~");
    }

    #[test]
    fn percent_decode_handles_lowercase_and_uppercase_hex() {
        assert_eq!(percent_decode("%2f%2F"), "//");
    }

    #[test]
    fn percent_decode_leaves_invalid_percent_sequences_alone() {
        assert_eq!(percent_decode("%GG"), "%GG");
        assert_eq!(percent_decode("end%"), "end%");
    }
}
