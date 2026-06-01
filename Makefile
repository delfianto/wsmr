.PHONY: test test-unit test-linux test-integration fmt clippy coverage coverage-unit

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

# Authoritative merged coverage (unit + Tier-B integration, one instrumented
# Linux build) with the >=90% gate. Needs podman.
coverage:
	./scripts/coverage.sh merged

# Fast native subset (unit tests only; no cfg(linux)/live-systemd paths).
coverage-unit:
	./scripts/coverage.sh unit
