//! Admin engine demo. Boots the framework with the new admin engine
//! mounted at `/admin`. Two demo models are registered automatically
//! by the framework — `users` (a hand-written `AdminUiModel`) and
//! `orders` (a `register_generated` config-driven model). This demo
//! seeds both tables with sample rows so the lists are not empty on
//! first load.
//!
//! Run:
//!   cargo run --example admin_demo -p rustio-core
//!
//! Open http://127.0.0.1:3000/admin and sign in as
//! admin@example.com / admin.

use std::net::SocketAddr;

use rustio_core::admin::Admin;
use rustio_core::auth::{self, authenticate};
use rustio_core::defaults::with_defaults;
use rustio_core::{Db, Router, Server};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let db = Db::memory().await.expect("db connect");
    auth::ensure_core_tables(&db)
        .await
        .expect("create auth tables");

    auth::user::create(&db, "admin@example.com", "admin", "admin")
        .await
        .expect("seed admin user");

    seed_demo_data(&db).await;

    let router = with_defaults(Router::new()).wrap(authenticate(db.clone()));
    // No `.model::<T>()` calls: the new engine uses its own
    // `AdminUiModel` registry (Users + Orders are wired in by
    // `Admin::register`). The legacy AdminModel + macro path is
    // intentionally not exercised here so there is exactly one
    // admin surface in the demo.
    let router = Admin::new().register(router, &db);

    let addr: SocketAddr = ([127, 0, 0, 1], 3000).into();
    eprintln!("admin demo: open http://{addr}/admin and sign in as admin@example.com / admin");
    Server::bind(addr).serve_router(router).await
}

/// Insert sample rows into the demo tables so the admin index isn't
/// empty on first boot. Tables are auto-created by each model's
/// `ensure_table_sql()` on first request, so we run them here too
/// to make seeding deterministic regardless of which page is hit
/// first.
///
/// Uses `Db::execute` with literal SQL because the framework keeps
/// sqlx hidden from user code (no `pool()` access from outside the
/// crate). All seed values are compile-time string literals — no
/// untrusted input — so literal interpolation is safe here.
async fn seed_demo_data(db: &Db) {
    db.execute(
        "CREATE TABLE IF NOT EXISTS admin_new_demo_users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT NOT NULL,
            email TEXT NOT NULL,
            is_active TEXT NOT NULL DEFAULT 'false',
            doctor_id TEXT,
            salary_amount TEXT
        )",
    )
    .await
    .expect("create users table");

    db.execute(
        "CREATE TABLE IF NOT EXISTS admin_new_demo_orders (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            order_number TEXT,
            customer_email TEXT,
            total_amount TEXT,
            is_paid TEXT
        )",
    )
    .await
    .expect("create orders table");

    let user_inserts = "INSERT INTO admin_new_demo_users \
         (username, email, is_active, doctor_id, salary_amount) VALUES \
         ('amansour',  'abdulwahed@rustio.dev', 'true',  '1', '5400'), \
         ('b.hassan',  'bashayer@rustio.dev',   'true',  '1', '4800'), \
         ('sara_m',    'sara@rustio.dev',       'true',  '2', '3600'), \
         ('l.nguyen',  'linh@example.com',      'true',  '1', '5100'), \
         ('k.ito',     'kenji@example.com',     'true',  '2', '3200'), \
         ('m.osei',    'maya@example.com',      'true',  '2', '3000'), \
         ('r.silva',   'rafael@example.com',    'false', '1', '2800'), \
         ('p.kapoor',  'priya@example.com',     'true',  '2', '4200'), \
         ('n.eriksson','nils@example.com',      'true',  '1', '3900'), \
         ('z.ahmed',   'zainab@example.com',    'false', '2', '2100')";
    let _ = db.execute(user_inserts).await;

    let order_inserts = "INSERT INTO admin_new_demo_orders \
         (order_number, customer_email, total_amount, is_paid) VALUES \
         ('RIO-1001', 'abdulwahed@rustio.dev', '129.00', 'true'), \
         ('RIO-1002', 'bashayer@rustio.dev',   '89.50',  'true'), \
         ('RIO-1003', 'sara@rustio.dev',       '245.00', 'false'), \
         ('RIO-1004', 'linh@example.com',      '320.00', 'true'), \
         ('RIO-1005', 'kenji@example.com',     '57.25',  'false'), \
         ('RIO-1006', 'maya@example.com',      '412.10', 'true'), \
         ('RIO-1007', 'rafael@example.com',    '76.40',  'false'), \
         ('RIO-1008', 'priya@example.com',     '199.99', 'true'), \
         ('RIO-1009', 'nils@example.com',      '88.00',  'true'), \
         ('RIO-1010', 'zainab@example.com',    '33.00',  'false')";
    let _ = db.execute(order_inserts).await;
}
