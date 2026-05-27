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

/// Copy `src` to `dest` verbatim, then print a `cargo:warning`
/// explaining why Tailwind was skipped. The build still succeeds —
/// browsers render the unprocessed CSS file fine (component selectors
/// work; only Tailwind-generated utilities and `@theme {}` token
/// expansion go missing).
fn passthrough(src: &Path, dest: &Path, reason: &str) {
    println!("cargo:warning=admin.css served unprocessed: {reason}");
    std::fs::copy(src, dest).unwrap_or_else(|e| {
        panic!(
            "failed to copy {} to {}: {e}",
            src.display(),
            dest.display()
        )
    });
}
