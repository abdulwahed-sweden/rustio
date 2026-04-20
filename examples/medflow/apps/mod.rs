use rustio_core::admin::Admin;
use rustio_core::{Db, Router};

// -- modules --
pub mod people;
pub mod care;
pub mod billing;
// -- end modules --

/// Build the admin registry.
///
/// Split from [`register_all`] so `main.rs --dump-schema` can introspect
/// the admin model list without touching the database or binding a port.
#[allow(unused_mut)]
pub fn build_admin() -> Admin {
    let mut admin = Admin::new();
    // -- admin installs --
    admin = people::admin::install(admin);
    admin = care::admin::install(admin);
    admin = billing::admin::install(admin);
    // -- end admin installs --
    admin
}

#[allow(unused_mut, unused_variables)]
pub fn register_all(mut router: Router, db: &Db) -> Router {
    router = build_admin().register(router, db);

    // -- view registrations --
    router = people::views::register(router);
    router = care::views::register(router);
    router = billing::views::register(router);
    // -- end view registrations --
    router
}
