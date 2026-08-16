.PHONY: build check test fmt clippy migrate provision-admin smoke manifests manifests-dryrun compose-up compose-down

build:
	cargo build --release --manifest-path app/Cargo.toml

check:
	cargo check --manifest-path app/Cargo.toml

test:
	cargo test --manifest-path app/Cargo.toml

fmt:
	cargo fmt --manifest-path app/Cargo.toml --all

fmt-check:
	cargo fmt --manifest-path app/Cargo.toml --all -- --check

clippy:
	cargo clippy --manifest-path app/Cargo.toml --all-targets -- -D warnings

ci: fmt-check clippy test build

migrate:
	ANAMNESIS_RUN_MIGRATIONS=1 cargo run --manifest-path app/Cargo.toml

provision-admin:
	@test -n "$$HOSPITAL_ID" -a -n "$$USERNAME" -a -n "$$PASSWORD" || (echo "set HOSPITAL_ID, USERNAME, PASSWORD"; exit 1)
	DATABASE_URL=$${DATABASE_URL:-postgres://postgres:root@localhost:5432/anamnesis?sslmode=disable} \
	HOSPITAL_ID=$$HOSPITAL_ID USERNAME=$$USERNAME PASSWORD=$$PASSWORD \
	NAME=$${NAME:-$$USERNAME} \
	cargo run --manifest-path app/Cargo.toml --bin provision_admin

dev: compose

compose:
	docker compose up -d --build

compose-down:
	docker compose down -v

smoke:
	bash scripts/smoke.sh

manifests-dryrun:
	kubectl apply --dry-run=client -k k8s/

manifests:
	kubectl apply -k k8s/

install-git-hooks:
	cp scripts/commit-hook .git/hooks/pre-commit
	chmod +x .git/hooks/pre-commit
	@echo "pre-commit hook installed"
