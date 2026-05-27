//! HTML-escape helper used by Rust code that still concatenates
//! small fragments outside the main minijinja templates (e.g. FK
//! cell links inside `list_render`, form-field controls inside
//! `form_render`). Every shell element — topbar, sidebar, page
//! header — is rendered through templates as of 0.10.
//!
//! When templates can express something completely, prefer the
//! template path over adding helpers here.

/// HTML-escape user-supplied text for interpolation into attributes
/// and text content. Handles `&`, `<`, `>`, `"`, `'`.
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_escape_handles_specials() {
        assert_eq!(
            html_escape("a & b < c > d \"e\" 'f'"),
            "a &amp; b &lt; c &gt; d &quot;e&quot; &#39;f&#39;"
        );
    }
}
