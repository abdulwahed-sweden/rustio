use rustio_core::admin::Admin;
use rustio_core::{Db, Router};

// -- modules --
pub mod assignments;
pub mod bookings;
pub mod customers;
pub mod invoices;
pub mod locations;
pub mod resources;
pub mod schedules;
// -- end modules --

/// Build the admin registry.
///
/// Split from [`register_all`] so `main.rs --dump-schema` can introspect
/// the admin model list without touching the database or binding a port.
#[allow(unused_mut)]
pub fn build_admin() -> Admin {
    let mut admin = Admin::new();
    // -- admin installs --
    admin = assignments::admin::install(admin);
    admin = bookings::admin::install(admin);
    admin = customers::admin::install(admin);
    admin = invoices::admin::install(admin);
    admin = locations::admin::install(admin);
    admin = resources::admin::install(admin);
    admin = schedules::admin::install(admin);
    // -- end admin installs --
    admin
}

#[allow(unused_mut, unused_variables)]
pub fn register_all(mut router: Router, db: &Db) -> Router {
    router = build_admin().register(router, db);

    // -- view registrations --
    router = assignments::views::register(router);
    router = bookings::views::register(router);
    router = customers::views::register(router);
    router = invoices::views::register(router);
    router = locations::views::register(router);
    router = resources::views::register(router);
    router = schedules::views::register(router);
    // -- end view registrations --
    router
}
