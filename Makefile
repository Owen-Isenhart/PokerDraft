.PHONY: profile security setup build run dev check browser simulate smoke format
setup:
	npm ci --prefix web
build:
	npm run build --prefix web
	cargo build --release --workspace --examples
	cargo build --release --workspace
run: build
	./target/release/pokerdraft-server
dev:
	bash scripts/dev.sh
check:
	python3 -m unittest discover -s sdk/python -p 'test_*.py'
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --all-features -- -D warnings
	cargo test --workspace --release
	npm run build --prefix web
browser:
	npm exec --prefix web -- playwright test -c web/playwright.config.ts
simulate:
	cargo run --release -p pokerdraft-engine --example simulate -- 2000
smoke:
	python3 scripts/smoke.py
format:
	cargo fmt --all
	npm exec --prefix web -- prettier --write 'web/src/*' 'web/*.{json,ts,html}' 'web/tests/*.ts' sdk/typescript/bot.ts

security:
	python3 scripts/security_checks.py

# Isolated temporary 1,000-account/archive fixture; no live database touched.
profile:
	cargo run --release -p pokerdraft-server --features profiling --bin profile -- 1000
