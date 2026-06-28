use rustio_core::admin::Admin;
use rustio_core::{Db, Router};

// -- modules --
pub mod appointments;
pub mod clients;
pub mod orders;
pub mod services;
pub mod staff;
// -- end modules --

/// Build the admin registry. Split from [`register_all`] so
/// `main.rs --dump-schema` can introspect the model list without a DB.
#[allow(unused_mut)]
pub fn build_admin() -> Admin {
    let mut admin = Admin::new();
    // -- admin installs --
    admin = appointments::admin::install(admin);
    admin = clients::admin::install(admin);
    admin = orders::admin::install(admin);
    admin = services::admin::install(admin);
    admin = staff::admin::install(admin);
    // -- end admin installs --
    admin
}

#[allow(unused_mut, unused_variables)]
pub fn register_all(mut router: Router, db: &Db) -> Router {
    router = build_admin().register(router, db);

    // -- view registrations --
    router = appointments::views::register(router);
    router = clients::views::register(router);
    router = orders::views::register(router);
    router = services::views::register(router);
    router = staff::views::register(router);
    // -- end view registrations --
    router
}
