# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## RustIO Engineering Specification (STRICT MODE)

Project Codename: Rustio
Default Location: Stockholm, Sweden
Default Timezone: Europe/Stockholm (UTC+1 / UTC+2 DST)
Language: English ONLY (No exceptions inside codebase or documentation)

## ❗ CRITICAL DIRECTIVE

This project is NOT a Django clone.
This is a new generation web framework inspired by Django's productivity but redesigned for:

- Rust performance
- Zero-runtime magic
- Compile-time guarantees
- Developer-first experience

Any attempt to:

- blindly copy Django
- reintroduce Python-style runtime behavior
- over-engineer configs

➡️ MUST BE REJECTED

## 🧠 Core Vision

"Django simplicity. Rust power. Zero bullshit."

RustIO is designed to:

- Feel like Django when starting
- Feel like Rust when scaling
- Feel like a product, not a framework

## 🚀 Installation Philosophy

```bash
cargo install rustio
```

Then immediately:

```bash
rustio new project mysite
cd mysite
rustio run
```

NO config files.
NO setup steps.
NO environment pain.

## 🧱 CLI Design (NON-NEGOTIABLE)

RustIO CLI replaces Django commands:

| Django Command                     | RustIO Equivalent           |
| ---------------------------------- | --------------------------- |
| `django-admin startproject`        | `rustio new project <name>` |
| `python manage.py startapp`        | `rustio new app <name>`     |
| `python manage.py runserver`       | `rustio run`                |
| `python manage.py makemigrations`  | `rustio migrate generate`   |
| `python manage.py migrate`         | `rustio migrate apply`      |
| `python manage.py createsuperuser` | `rustio admin create`       |

## 🏗️ Project Structure (Generated)

```text
mysite/
├── main.rs
├── apps/
│   └── blog/
│       ├── models.rs
│       ├── views.rs
│       └── admin.rs
├── core/
├── static/
├── templates/
└── rustio.toml (OPTIONAL — discouraged)
```

## 🌐 Default Behavior (MANDATORY)

When user runs project:

```bash
rustio run
```

System MUST:

- Start HTTP server
- Auto-create database (if missing)
- Apply migrations
- Serve default homepage

## 🏠 Default Homepage (STRICT REQUIREMENT)

RustIO MUST generate a clean minimal homepage:

### UI Requirements

- No clutter
- Centered layout
- Two buttons ONLY:
  - `[ Go to Admin ]`
  - `[ Go to Documentation ]`

### Behavior

- `/admin` → Admin Panel
- `/docs` → Local documentation page

### Purpose

- Give instant feedback
- Reduce confusion
- Improve first impression

## 🧩 Architecture (FROM ZERO — NO FRAMEWORKS)

🚫 DO NOT USE:

- Axum
- Actix
- Rocket

✅ YOU MUST BUILD:

### 1. HTTP Layer

Based on:

- `hyper` (preferred)
- OR raw TCP (advanced mode)

### 2. Router (Custom)

Must support:

```text
GET /users
POST /users
```

With:

- pattern matching
- typed params

### 3. ORM Layer (STRICT)

DO NOT expose SQLx directly.

RustIO must provide:

```rust
User::create(...)
User::find(...)
User::all()
```

Internally:

- can use SQLx
- but hidden completely

### 4. Admin System (CORE FEATURE)

This is NOT optional.

Must be generated via macros:

```rust
#[derive(RustioAdmin)]
struct User {
    id: i32,
    name: String,
    is_active: bool,
}
```

MUST generate:

- Full CRUD UI
- Forms
- Tables
- Validation

## ⚙️ Compile-Time Philosophy

RustIO MUST:

- generate admin at compile time
- validate models at compile time
- reduce runtime logic to minimum

## 📦 Binary Strategy

Final output MUST be:

```text
./app
```

Single binary containing:

- server
- database connection
- templates
- CSS
- JS

Use:

- `include_str!`
- `include_bytes!`

## 🎨 Admin Panel (YOU MUST IMPROVE DJANGO)

Django admin is:

- ❌ ugly
- ❌ outdated
- ❌ slow

RustIO admin MUST be:

- ✅ modern
- ✅ minimal
- ✅ fast
- ✅ responsive

### Suggested Stack

- HTML + minimal JS
- OR WASM (advanced later)

## 🧠 Developer Experience Rules

RustIO must feel:

- instant
- predictable
- clean

## ❌ STRICT PROHIBITIONS

Claude Code MUST NOT:

- add unnecessary abstractions
- introduce config-heavy systems
- depend on heavy frameworks
- overcomplicate architecture
- mimic Django internals blindly

## ✅ ACCEPTABLE TRADE-OFFS

- Less features initially ✔
- More control ✔
- Simplicity over completeness ✔

## 🗺️ Roadmap (REALISTIC)

### Phase 1 (Core)

- CLI
- HTTP server
- Router
- Basic ORM
- Homepage

### Phase 2

- Admin system (macro-based)
- Migrations
- Auth

### Phase 3

- Performance optimization
- Plugin system

## ⚠️ FINAL WARNING TO CLAUDE CODE

If your implementation:

- feels like Django → ❌ WRONG
- feels like config hell → ❌ WRONG
- feels slow → ❌ WRONG

If it feels:

- fast
- simple
- powerful

➡️ THEN you're on the right track.

## 🧭 Default Environment

- City: Stockholm
- Timezone: Europe/Stockholm
- Locale: en_US (default)

Must be configurable later — but NOT required initially.

## 🔚 Final Statement

RustIO is not a library.

It is:

**A developer experience engine.**
