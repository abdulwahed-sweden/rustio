//! The first-run journey, end to end, against the real `rustio` binary.
//!
//! This is the one test that answers "does a stranger's first ten
//! minutes still work?" — the path every other test only covers a slice
//! of. It scaffolds a project in a temp directory and walks it:
//!
//!   init booklend → add model book → migrate apply → user create → doctor
//!
//! Two things make it affordable to run in the normal suite:
//!
//!   * `RUSTIO_CORE_PATH` points the generated `Cargo.toml` at this
//!     workspace's `rustio-core`, so the test never depends on what is
//!     published to crates.io;
//!   * `CARGO_TARGET_DIR` points the child build at this workspace's
//!     `target/`, so the dependencies it needs are already compiled.
//!
//! The first run on a cold machine still pays for one project build
//! (`migrate apply` shells out to `cargo run -- --dump-schema`, which is
//! how `rustio.schema.json` is produced at all). Nothing is left behind:
//! no server is started and the temp directory is removed on success.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Absolute path to the CLI binary cargo just built for this test.
fn rustio() -> &'static str {
    env!("CARGO_BIN_EXE_rustio")
}

fn workspace_root() -> PathBuf {
    // rustio-cli/ → workspace root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rustio-cli has a parent directory")
        .to_path_buf()
}

/// Process-private scratch directory. Not `tempfile` — the workspace
/// keeps its dependency list short on purpose.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rustio-journey-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Run the CLI in `cwd` and return its output, failing the test with
/// both streams attached when the command exits non-zero.
fn run(cwd: &Path, args: &[&str]) -> Output {
    let root = workspace_root();
    let out = Command::new(rustio())
        .args(args)
        .current_dir(cwd)
        // Generated projects point at this checkout, not crates.io.
        .env("RUSTIO_CORE_PATH", root.join("rustio-core"))
        // Reuse the workspace's compiled dependencies for the child build.
        .env("CARGO_TARGET_DIR", root.join("target"))
        // Stable, un-styled output to assert against.
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn `rustio {}`: {e}", args.join(" ")));

    assert!(
        out.status.success(),
        "`rustio {}` exited with {:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        args.join(" "),
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    out
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn first_run_journey_scaffolds_migrates_and_passes_doctor() {
    let tmp = scratch("first-run");

    // 1. Create the project. stdin is not a terminal here, so `init`
    //    skips the setup menu and prints the closing screen directly.
    let out = run(&tmp, &["init", "booklend"]);
    assert!(
        stdout(&out).contains("Created project"),
        "init should report the project it created:\n{}",
        stdout(&out)
    );

    let project = tmp.join("booklend");
    assert!(project.join("models/mod.rs").is_file(), "models/mod.rs");
    assert!(
        !project.join("apps").exists(),
        "a new project must never get an `apps/` directory"
    );

    // 2. One model.
    let out = run(&project, &["add", "model", "book"]);
    let text = stdout(&out);
    assert!(text.contains("Created model Book"), "{text}");
    assert!(text.contains("models/book/models.rs"), "{text}");
    assert!(
        project.join("models/book/models.rs").is_file(),
        "the model file is written where the output says it is"
    );
    assert!(
        !project.join("apps").exists(),
        "adding a model must never create an `apps/` directory"
    );

    // 3. Create the tables. This also regenerates rustio.schema.json by
    //    building the project — the only path that produces it.
    let out = run(&project, &["migrate", "apply"]);
    let text = stdout(&out);
    assert!(text.contains("Applied 1 migration"), "{text}");
    assert!(text.contains("schema updated (1 model + User)"), "{text}");

    let schema = std::fs::read_to_string(project.join("rustio.schema.json"))
        .expect("migrate apply regenerates rustio.schema.json");
    assert!(
        schema.contains("\"Book\""),
        "the exported schema must carry the model that was added:\n{schema}"
    );

    // 4. Someone to sign in as.
    let out = run(
        &project,
        &[
            "user",
            "create",
            "--email",
            "admin@booklend.local",
            "--password",
            "demo1234",
            "--role",
            "admin",
        ],
    );
    assert!(
        stdout(&out).contains("Created user admin@booklend.local (admin)"),
        "{}",
        stdout(&out)
    );

    // 5. A healthy project. The port check is deliberately not asserted:
    //    whether :8000 is free is a property of the machine, not of the
    //    journey.
    let text = stdout(&run(&project, &["doctor"]));
    for expected in [
        "Project structure",
        "Models registered",
        "Database file",
        "Schema export",
        "all migrations applied",
        "schema matches models",
        "1 admin user",
    ] {
        assert!(
            text.contains(expected),
            "doctor should report `{expected}`:\n{text}"
        );
    }
    assert!(
        !text.contains("no admin users") && !text.contains("pending"),
        "doctor should find nothing outstanding:\n{text}"
    );

    // 6. The bare command reads the project without changing it.
    let text = stdout(&run(&project, &[]));
    assert!(text.contains("booklend · 1 model"), "{text}");
    assert!(text.contains("0 pending migrations"), "{text}");

    let _ = std::fs::remove_dir_all(&tmp);
}
