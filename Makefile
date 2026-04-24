DB_URL ?= postgres://postgres:dev@localhost/rustio_dev

.PHONY: up down db-setup migrate run check clean

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
