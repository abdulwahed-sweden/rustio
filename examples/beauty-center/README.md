# Beauty Center — RustIO example

A small but realistic beauty/salon center: clients, services, staff,
appointments, and product orders. Five models with varied field types so every
admin feature (the four list layouts, the composition editor, status badges +
filters, foreign-key links, and the i18n language switch) has something to show.

Backend names are English (the iron rule); the UI translates to Swedish.

See the repo's run guide, or:

    # from the repo root
    cd examples/beauty-center
    RUSTIO_CORE_PATH="$(pwd)/../../rustio-core" cargo run --manifest-path ../../Cargo.toml -p rustio-cli -- migrate apply
    RUSTIO_CORE_PATH="$(pwd)/../../rustio-core" cargo run --manifest-path ../../Cargo.toml -p rustio-cli -- user create --email admin@beauty.local --password demo1234 --role admin
    cargo run
    # open http://127.0.0.1:8000/admin  (admin@beauty.local / demo1234)
