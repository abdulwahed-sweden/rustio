//! Compile-time admin.css build step.
//!
//! Runs Tailwind v4 over `assets/static/admin.css`, writing the compiled
//! result to `$OUT_DIR/admin.css` where `templating.rs` `include_bytes!`'s
//! it into the binary.
//!
//! Tailwind discovery order:
//!  1. `tailwindcss` standalone binary in PATH
//!     (install via `brew install tailwindcss` on macOS, or download
//!     the standalone release from
//!     <https://github.com/tailwindlabs/tailwindcss/releases>).
//!  2. `npx -y @tailwindcss/cli` if Node is available.
//!  3. Passthrough — copy the source CSS to `OUT_DIR` unchanged. A
//!     `cargo:warning` is printed so developers know they're getting a
//!     degraded build (no Tailwind utility generation; `@theme {}`
//!     tokens are ignored by browsers and need a `:root {}` mirror to
//!     remain useful).
//!
//! Override with `RUSTIO_SKIP_TAILWIND=1` to force passthrough (useful
//! in CI environments where Tailwind isn't installed and the bundled
//! source CSS is known to be self-contained).

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

const SRC_REL: &str = "assets/static/admin.css";
const TEMPLATES_REL: &str = "assets/templates";

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let src = manifest.join(SRC_REL);
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let dest = out_dir.join("admin.css");

    println!("cargo:rerun-if-changed={SRC_REL}");
    println!("cargo:rerun-if-changed={TEMPLATES_REL}");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RUSTIO_SKIP_TAILWIND");

    if env::var_os("RUSTIO_SKIP_TAILWIND").is_some() {
        passthrough(&src, &dest, "RUSTIO_SKIP_TAILWIND set");
        return;
    }

    if try_run("tailwindcss", &compile_args(&src, &dest)) {
        return;
    }
    if try_run("npx", &npx_args(&src, &dest)) {
        return;
    }

    passthrough(
        &src,
        &dest,
        "no tailwindcss binary or npx found in PATH; \
         install via `brew install tailwindcss` (single binary) or \
         `npm i -g @tailwindcss/cli`, or set RUSTIO_SKIP_TAILWIND=1",
    );
}

fn compile_args(src: &Path, dest: &Path) -> Vec<String> {
    vec![
        "-i".into(),
        src.display().to_string(),
        "-o".into(),
        dest.display().to_string(),
        "--minify".into(),
    ]
}

fn npx_args(src: &Path, dest: &Path) -> Vec<String> {
    let mut args = vec!["-y".into(), "@tailwindcss/cli".into()];
    args.extend(compile_args(src, dest));
    args
}

/// Run a command, returning `true` on success. Spawn failures (binary
/// not on PATH, permission denied) are silent — the next fallback in
/// the chain takes over. Non-zero exit prints a `cargo:warning` so
/// real Tailwind errors aren't swallowed.
fn try_run(cmd: &str, args: &[String]) -> bool {
    let result = Command::new(cmd).args(args).status();
    match result {
        Ok(status) if status.success() => true,
        Ok(status) => {
            println!(
                "cargo:warning={cmd} exited with {status}; falling through to the next compile strategy"
            );
            false
        }
        Err(_) => false,
    }
}

/// Copy `src` to `dest` with two passthrough-mode rewrites:
///   - strip any `@import "tailwindcss"` line so browsers don't 404
///     trying to resolve it as a stylesheet URL;
///   - drop the entire `@theme { … }` block (Tailwind-only syntax;
///     the equivalent variables are already declared inside `:root`).
///
/// Both rewrites are no-ops when the source CSS happens not to contain
/// those constructs, so plain CSS sources pass through verbatim.
fn passthrough(src: &Path, dest: &Path, reason: &str) {
    println!("cargo:warning=admin.css served unprocessed: {reason}");
    let source = std::fs::read_to_string(src)
        .unwrap_or_else(|e| panic!("failed to read {} for passthrough: {e}", src.display()));
    let stripped = strip_tailwind_only_syntax(&source);
    std::fs::write(dest, stripped)
        .unwrap_or_else(|e| panic!("failed to write {} for passthrough: {e}", dest.display()));
}

/// Remove the two constructs that only Tailwind understands —
/// `@import "tailwindcss"` and `@theme { … }` — so the file is
/// browser-clean. Variable definitions in `:root` are expected to
/// mirror anything that `@theme` would have published, so dropping
/// the block has no runtime effect.
fn strip_tailwind_only_syntax(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '@' {
            let rest = &source[i..];
            if let Some(rest_after) = rest.strip_prefix("@import") {
                let trimmed = rest_after.trim_start();
                if trimmed.starts_with("\"tailwindcss\"") || trimmed.starts_with("'tailwindcss'") {
                    if let Some(end) = source[i..].find(';') {
                        for _ in 0..end {
                            chars.next();
                        }
                        chars.next(); // consume the ';'
                        continue;
                    }
                }
            }
            if rest.starts_with("@theme") {
                if let Some(brace_offset) = rest.find('{') {
                    let abs_brace = i + brace_offset;
                    let mut depth = 0i32;
                    let bytes = source.as_bytes();
                    let mut idx = abs_brace;
                    while idx < bytes.len() {
                        let ch = bytes[idx] as char;
                        if ch == '{' {
                            depth += 1;
                        } else if ch == '}' {
                            depth -= 1;
                            if depth == 0 {
                                idx += 1;
                                break;
                            }
                        }
                        idx += 1;
                    }
                    // Step the outer iterator past the entire `@theme { … }`
                    // block. `chars` is already positioned one past the
                    // initial `@`, so advance by (idx - i - 1) more chars.
                    let consumed = idx - i;
                    for _ in 0..consumed.saturating_sub(1) {
                        chars.next();
                    }
                    continue;
                }
            }
        }
        out.push(c);
    }
    out
}
