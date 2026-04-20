# rustio-core

Runtime core for the [RustIO](https://github.com/abdulwahed-sweden/rustio) web framework.

Provides the HTTP server, router, middleware chain, request context, error handling, ORM, admin UI, migrations, the AI planner/review/executor pipeline, the context layer, and the admin intelligence module.

Normally used indirectly via the `rustio-cli` binary, which scaffolds projects that depend on this crate.

```
cargo install rustio-cli
rustio new project mysite
```

See the [main repository](https://github.com/abdulwahed-sweden/rustio) for documentation.

## License

MIT
