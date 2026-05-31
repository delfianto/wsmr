.PHONY: test test-unit test-linux test-integration fmt clippy

# Unit tests (platform-neutral logic) — runs natively, including on macOS.
test-unit:
	cargo test

# Build + unit tests inside a Linux container (Tier A).
test-linux:
	./scripts/linux-test.sh $(FILTER)

# Full session-bootstrap integration test on real systemd (Tier B).
test-integration:
	./scripts/linux-integration.sh

# Everything (unit + Linux build/test + integration).
test: test-unit test-linux test-integration

fmt:
	cargo fmt

clippy:
	cargo clippy --all-targets --all-features -- -D warnings
