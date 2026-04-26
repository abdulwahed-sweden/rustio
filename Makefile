DB_URL ?= postgres://postgres:dev@localhost/rustio_dev

.PHONY: up down db-setup migrate run check clean css css-watch css-check

up:
	docker compose up -d
	@printf "waiting for postgres + meilisearch to be healthy"
	@for i in $$(seq 1 60); do \
		pg=$$(docker inspect -f '{{.State.Health.Status}}' $$(docker compose ps -q postgres) 2>/dev/null); \
		ms=$$(docker inspect -f '{{.State.Health.Status}}' $$(docker compose ps -q meilisearch) 2>/dev/null); \
		if [ "$$pg" = "healthy" ] && [ "$$ms" = "healthy" ]; then \
			echo " — ok"; exit 0; \
		fi; \
		printf "."; sleep 1; \
	done; \
	echo; echo "services did not become healthy in time"; exit 1

down:
	docker compose down

db-setup:
	@docker compose exec -T postgres psql -U postgres -tAc \
	    "SELECT 1 FROM pg_database WHERE datname='rustio_dev'" | grep -q 1 \
	  || docker compose exec -T postgres psql -U postgres -c "CREATE DATABASE rustio_dev"

migrate:
	cargo run -p rustio-cli -- migrate apply \
		--db $(DB_URL) \
		--dir examples/blog/migrations

run:
	cargo run -p blog

check:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace

clean:
	cargo clean
	docker compose down -v

# --- Phase 7a/2: Tailwind build pipeline ----------------------------------
# `make css` regenerates rustio-core/assets/static/css/admin.css from the
# Tailwind source in rustio-core/assets/css/input.css. The output IS
# committed (so `cargo run` works for anyone without Node), but anyone
# editing styles needs Node + `npm install` first. `make css-check`
# fails if the committed CSS is out of sync with the input — wire it
# into a pre-commit hook if you want.

css:
	@if [ ! -d node_modules ]; then \
		echo "node_modules missing — run 'npm install' first"; exit 1; \
	fi
	npx tailwindcss -i rustio-core/assets/css/input.css -o rustio-core/assets/static/css/admin.css --minify

css-watch:
	@if [ ! -d node_modules ]; then \
		echo "node_modules missing — run 'npm install' first"; exit 1; \
	fi
	npx tailwindcss -i rustio-core/assets/css/input.css -o rustio-core/assets/static/css/admin.css --watch

css-check:
	@if [ ! -d node_modules ]; then \
		echo "node_modules missing — run 'npm install' first"; exit 1; \
	fi
	@npx tailwindcss -i rustio-core/assets/css/input.css -o /tmp/admin.css.expected --minify 2>/dev/null
	@if diff -q /tmp/admin.css.expected rustio-core/assets/static/css/admin.css > /dev/null; then \
		echo "css in sync"; \
	else \
		echo "ERROR: rustio-core/assets/static/css/admin.css is out of date — run 'make css'"; exit 1; \
	fi
