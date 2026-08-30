# Testing

wsmr has fast native tests and container tests that exercise a real systemd
instance. They answer different questions.

## Native tests

```sh
just test
# equivalent to:
cargo test
```

At the time of this documentation audit, the suite contains 248 passing Rust
tests: 229 library tests, 18 binary tests, and one uwsm unit-compatibility
test. Use the command output, not this count, as the long-term source of truth.

The compatibility test compares generated static units with files captured
from an actual uwsm 0.26.7 installation.

## Tier A: reproducible Linux build

```sh
just test-linux          # optional test-name filter may follow
just build-linux
```

Tier A builds in a pinned Debian Podman image. It catches dependency and host
toolchain assumptions. It does not boot a real user session.

## Tier B: systemd integration

```sh
just integration
```

Tier B boots systemd as PID 1 in a container, starts a user manager, and runs
the full session lifecycle with a stub compositor. The happy-path script also
checks application scopes, desktop-entry expansion, terminal wrapping,
multi-instance launching, duplicate-start rejection, environment restoration,
and the app-daemon protocol.

Six failure scripts run separately, each on a fresh container boot:

- crash before readiness;
- readiness timeout;
- unclean compositor exit;
- environment preparation failure;
- interrupted start; and
- partial failure while finalizing the environment.

Run all six directly with:

```sh
scripts/linux-integration-failures.sh
```

The CI workflow runs the native gate, an MSRV build/test job, the Tier B happy
path, and all six failure scripts. The Tier B job is labeled informational.

## Coverage

```sh
just coverage-unit      # native subset
just coverage           # merged authoritative report
just coverage-html      # merged report plus HTML
```

The merged run instruments one build, then combines native tests with the
Tier B happy path inside a systemd container. It enforces at least 90% line
coverage. The current local report is above 92%, but the gate is not yet part
of CI.

Most wsmr processes finish with `exec()`, which normally skips LLVM's final
profile write. Coverage builds call `coverage::flush_before_exec()` before
those replacements, and propagate `LLVM_PROFILE_FILE` into the user manager.

## Real hardware

The disposable-user harness is separate from Tier B because a stub compositor
cannot test DRM, input seats, portals, or a display manager. See
[the hardware harness](../arch/e2e.md) and [Known issues](known-issues.md).
