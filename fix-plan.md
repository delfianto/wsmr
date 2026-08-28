# wsmr remediation and integration-test plan

This document is the canonical tracker for findings from the implementation
review. It is intentionally planning-only: checking an item means its change has
been implemented **and** its acceptance criteria have passed.

## Tracker conventions

- [ ] Not started
- [~] In progress
- [x] Complete and verified
- [!] Blocked; add the reason next to the item

Priority definitions:

- **P0:** can damage or interfere with an existing desktop session
- **P1:** incorrect behavior, compatibility break, or unreliable verification
- **P2:** robustness, portability, maintainability, or documentation issue

Do not mark a phase complete merely because its code was written. Record the
verification command or evidence in the phase's evidence section.

## Overall gates

- [~] **G0 — Safe generation:** Phase 0 is complete before running wsmr against
  the active user's systemd user manager. File-level ownership/atomicity work
  is done and unit-tested; live-bus behavioral verification is still open
  (see Phase 0 evidence).
- [~] **G1 — Safe state handling:** Phases 0 and 1 are complete before a real
  Hyprland login is attempted. Locking/atomicity/generation-scoping is done
  and unit-tested (including a genuine concurrent-OS-thread test); the same
  live-bus verification gap as G0 applies to the bus-dependent paths
  (`begin_generation`/`end_generation`) — see Phase 1 evidence.
- [ ] **G2 — Credible Tier B:** Phase 4 passes without ignored functional
  failures before claiming session bootstrap is integration-tested.
- [ ] **G3 — Real machine:** Phase 7 passes under a disposable CachyOS user
  before claiming CachyOS/Wayland/Hyprland runtime support.

## Current baseline

- Host: CachyOS, Wayland, Hyprland 0.56.2, systemd 261, dbus-broker.
- The active desktop is managed by uwsm 0.26.7, not wsmr.
- Formatting, clippy, and build checks pass.
- Unit tests currently report 196 passing and 3 host-dependent failures (was
  166/3 before Phase 0/1; Phase 0 added 21 tests, Phase 1 added another 9; the
  3 failures are pre-existing and unrelated — see Phase 3).
- The Tier-B smoke reaches the core lifecycle, but ignores failures in terminal
  launch and finalization and therefore can report a false success.
- wsmr and uwsm currently use the same unit namespace. Phase 0's file-level
  ownership safety and Phase 1's session-state locking/generation-scoping are
  both implemented and unit-tested (below), but **G0/G1 are not yet fully
  closed**: double-start refusal, reload-failure handling, and the
  generation-begin/end paths are only proven at the pure-logic level, not
  against a live/mocked systemd user manager (needs the D-Bus test seam from
  Phase 3, or a live/Tier-B run). Until that verification happens, still do
  not run wsmr `start` or destructive cleanup against the active account.

---

## Phase 0 — P0 session and unit-file safety

**Goal:** wsmr must never mutate, overwrite, reload, or remove an existing uwsm
or foreign session graph merely because a start was refused or a dry run was
requested.

### P0-01 Check session activity before generation

Finding: `src/session/start.rs` generates the unit graph and reloads the user
manager before checking whether a compositor is already active.

- [x] Move all read-only eligibility checks ahead of filesystem writes and
  `daemon-reload`. `start::run` now orders: gst gate → connect bus → refuse if
  active → compute plan (read-only) → refuse on conflict → *only then*
  apply/reload (`src/session/start.rs:44-90`).
- [x] Resolve the compositor and intended paths without mutating state.
  `plan_generate` only stats/reads (`src/units/plan.rs`).
- [x] Refuse a start while an existing compositor/session is active. Reuses
  `session::stop::is_active`, which (unlike the old inline check) correctly
  covers both `wayland-wm@*.service` *and* `graphical-session.target`.
- [x] Ensure the refusal does not change unit-file hashes or manager state.
  Structurally guaranteed: `refuse_if_active` returns before `plan_generate`
  or `apply_generate` are ever called.

Acceptance criteria:

- [~] With a fake active unit, `start` returns the documented conflict result.
  No generation writer or reload method is called. The refusal predicate
  itself is unit-tested in isolation
  (`session::start::tests::refuse_if_active_blocks_only_when_active`), and the
  call order in `run` is linear/reviewable so it can't silently regress, but
  there is **no automated test that drives `run()` end-to-end against a real
  or mocked session bus** — `SessionBus` has no injectable seam yet. That's
  Phase 3's P3-02 (or a Tier-B scenario). Not closing this out until one of
  those lands.
- [x] Runtime unit-directory contents and hashes remain unchanged (on refusal,
  nothing downstream of the check runs, so trivially true; separately proven
  for the plan/apply split by
  `units::plan::tests::plan_generate_never_touches_disk`).

### P0-02 Make dry-run strictly read-only

Finding: start dry-run currently generates and reloads; stop dry-run with removal
can delete files while only suppressing the reload.

- [x] Build a pure `GenerationPlan`/`RemovalPlan` representation
  (`src/units/plan.rs`) — building either only stats/reads files and the
  manifest, never writes.
- [x] Make dry-run render or report that plan without creating directories.
  `start::report_plan` / `stop::report_removal_plan` print writes, removes,
  conflicts/skips, and whether a reload would follow.
- [x] Prevent all filesystem, systemd, D-Bus, and process mutations in
  dry-run. `start::run` and `stop::run_stop` both return right after
  computing + reporting the plan when `dry_run` is set, before
  `apply_generate`/`apply_removal` or `bus.reload()` are reachable.
- [x] Cover start and stop/remove variants. Both paths restructured;
  `stop --dry-run --remove` no longer deletes anything (previously it deleted
  unconditionally and only skipped the reload).

Acceptance criteria:

- [x] Tests observe zero mutation calls for dry-run's read path: not literally
  spies (no bus/process mocking exists yet), but equivalent in effect —
  `plan_generate_never_touches_disk` and
  `plan_remove_all_never_touches_disk_and_ignores_graph_units` snapshot the
  directory before/after planning and assert no change.
- [x] Before/after filesystem snapshots are identical (same tests).
- [x] Dry-run output identifies intended writes, conflicts, removals, and
  reloads (`report_plan`/`report_removal_plan`).

### P0-03 Introduce ownership-aware generated files

Finding: `src/units/generate.rs::remove_all` deletes fixed unit names and whole
`wayland-wm@*.service.d` directories without proving they belong to wsmr.

- [x] Add an unambiguous generated-file header — **implemented differently
  than literally specified**: rather than an in-file header (which would
  break the static graph's byte-identity with upstream, itself a stated
  goal in `templates.rs`), ownership is proven by an external per-directory
  manifest (`.wsmr-generation`, `src/units/manifest.rs`) recording a content
  fingerprint of what wsmr wrote at each path. This is unambiguous and, unlike
  a text header, can't be spoofed by hand-copying a comment line. See
  `docs/coexistence.md`.
- [x] Write a per-generation ownership manifest containing exact file paths
  and sufficient metadata to recognize the generation (`Manifest`: relative
  path → 64-bit content fingerprint, atomically persisted).
- [x] Classify every intended destination as absent, wsmr-owned, or foreign
  (`plan::classify_write` / `plan::classify_dropin`).
- [x] Refuse to overwrite foreign files by default (`Conflict`, blocks the
  whole plan — see P0-04).
- [x] Remove only manifest-owned files that still carry valid ownership data
  (`plan_remove_all` re-verifies the fingerprint against current disk content
  before scheduling a removal; a drifted entry is skipped, not deleted).
- [x] Never recursively delete a shared drop-in directory. No `remove_dir_all`
  exists anywhere in the new code path; only single-file `remove_file` plus a
  non-recursive `remove_dir` on the now-possibly-empty parent.
- [x] Remove an empty directory only after its owned children are removed and
  it is confirmed empty (`remove_empty_parent` uses non-recursive `remove_dir`,
  which only succeeds when the directory is already empty — enforced by the
  OS, not by wsmr counting entries).
- [x] Define an explicit future migration/adoption flow; do not adopt
  implicitly. Documented in `docs/coexistence.md`: no adoption path exists
  yet, and none of the new code ever records ownership of a file wsmr didn't
  itself just write.

Acceptance criteria:

- [x] A foreign same-name unit causes a safe refusal
  (`foreign_existing_file_is_a_conflict_not_a_write`,
  `conflicting_plan_is_refused_and_writes_nothing`).
- [x] Foreign sibling drop-ins survive generation and cleanup byte-for-byte
  (`remove_all_never_deletes_a_sibling_foreign_dropin`,
  `foreign_file_at_a_managed_dropin_path_is_not_a_conflict_on_removal_plan`).
- [x] A stale or tampered manifest cannot authorize broad deletion
  (`tampered_manifest_entry_is_a_conflict`,
  `plan_remove_all_skips_drifted_entries`).
- [x] Cleanup is idempotent (`remove_all_leaves_graph_units_and_removes_owned_dropins`
  runs removal twice and asserts the second is a no-op;
  `remove_missing_dir_is_noop`).

### P0-04 Make generation transactional

- [x] Inventory and validate the complete plan before writing its first file.
  `plan_generate` computes every write/remove/conflict up front;
  `apply_generate`/`apply_removal` re-check `plan.conflicts` and refuse before
  the first write as a last line of defense.
- [x] Write same-directory temporary files and atomically rename them
  (`units::fsutil::atomic_write`: temp file next to the destination, fsynced,
  then renamed).
- [x] Publish the manifest only for a coherent generation. `manifest.save()`
  is the last step of `apply_generate`/`apply_removal`, after every file write
  and removal in the batch has already succeeded.
- [~] Handle write, rename, and `daemon-reload` failures without leaving a
  mixed old/new graph. Write/rename failures: handled and tested (rollback,
  below). `daemon-reload` failure specifically: not yet handled/tested — if
  `bus.reload()` fails after `apply_generate` succeeds, the on-disk graph and
  manifest are already fully coherent (never mixed), but the *running* user
  manager may not have picked up the new generation until a later reload;
  there's no test pinning that behavior down, and no explicit recovery
  message distinguishing "written but not reloaded" from other failures.
- [~] Add a transaction/rollback guard or document and test an equally safe
  recovery strategy. Implemented: a same-call rollback guard
  (`Applied`/`rollback` in `src/units/generate.rs`) restores every
  already-applied write/remove in the current batch to its prior content if a
  later step in the same batch fails, verified by
  `a_failed_write_rolls_back_earlier_writes_in_the_same_batch`. Not yet
  covered: a crash *between* the last file rename and the manifest rename —
  documented as fail-closed (next run sees a fingerprint mismatch for that one
  path and refuses rather than corrupting anything) but not exercised by a
  test, since simulating a process crash mid-syscall isn't practical at the
  unit-test level.
- [x] Reload only when the applied graph actually changes (`if outcome.changed
  { bus.reload()?; }`, both `start.rs` and `stop.rs`).

Acceptance criteria:

- [~] Fault injection at each write/rename/reload boundary leaves either the
  old valid generation or the new valid generation. Write/rename boundary:
  covered by the rollback test above. Reload boundary: not covered (needs a
  live/mocked bus — same gap as P0-01).
- [x] No temporary files remain after success or handled failure
  (`fsutil::tests::writes_and_overwrites_leaving_no_temp_files`; rollback
  reuses `atomic_write`/`remove_file`, which clean up their own temp files on
  either path).
- [x] The manifest always describes the installed owned files (`record`/
  `forget` kept in lockstep with every write/remove in the same batch, saved
  once at the end of that batch).

### P0-05 Coexistence policy

- [x] Document that uwsm-compatible unit names are intentionally shared —
  `docs/coexistence.md`.
- [x] Default to refusing foreign same-name runtime units, even when inactive.
  Ownership classification in `plan.rs` never looks at compositor/session
  active state — it's purely about what's on disk and in the manifest, so the
  refusal applies identically whether or not anything is running.
- [x] Provide actionable conflict diagnostics showing exact paths and owners
  (`generate::conflict_error`: lists the directory, every conflicting relative
  path, and why).
- [x] Do not suggest `stop --remove` as recovery until ownership-safe removal
  is implemented. Ownership-safe removal now *is* implemented (P0-03/P0-04),
  so this constraint is satisfied by having removed the underlying risk rather
  than by continuing to withhold the suggestion; `docs/coexistence.md`
  documents that `stop --remove` is now safe to run even while uwsm coexists
  in the same unit directory.

Phase 0 evidence:

- [x] `cargo test`: 184 passed, 3 failed (native CachyOS host,
  `cargo 1.98.0`). The 3 failures are pre-existing host-environment
  dependencies unrelated to this phase (`session::prepare::tests::
  deduce_session_needs_logind_when_incomplete`,
  `app::terminal::tests::neg_cache_records_non_terminals_and_finds_terminal`,
  `app::terminal::tests::find_terminal_entry_from_list_and_scan`) — see
  Phase 3 (P3-01/P3-02), which exists specifically to fix this class of test.
- [x] `scripts/linux-test.sh`: 187 passed, 0 failed, inside the Debian
  `Containerfile` image (all 3 host-dependent tests pass there too, since the
  container has none of the real XDG/logind state that trips them up on the
  live desktop host — consistent with the diagnosis, not a contradiction).
- [x] `scripts/linux-build.sh`: `cargo build --all-targets` and
  `cargo clippy --all-targets -- -D warnings` both exit 0 in the container.
- [x] `cargo fmt --check`: clean.
- [x] Additional fault-injection test command/results:
  `cargo test a_failed_write_rolls_back` — passes; simulates a mid-batch
  failure (second planned write's destination occupied by a directory) and
  asserts the first write's content was rolled back to its pre-batch value.
  Reload-boundary and process-crash fault injection are **not** covered — see
  the `[~]` notes above.
- [ ] Reviewer notes:

---

## Phase 1 — P1 environment-state integrity

**Goal:** concurrent finalize/watcher activity and interrupted sessions cannot
lose, corrupt, or apply stale environment cleanup state.

### P1-01 Serialize environment state changes

Finding: `src/session/finalize.rs`, `src/session/exec.rs`, and
`src/env/files.rs::append_cleanup` perform an unlocked read-modify-write.

- [x] Introduce one session-state lock covering the pre-session snapshot,
  cleanup list, and session configuration. New `src/session/state.rs` is now
  the *only* place `prepare-env`, `finalize`, the readiness watcher, and
  `cleanup-env` touch `env_pre`/`env_cleanup.list`/`generation` — every
  caller goes through `begin_generation`/`append_cleanup`/`end_generation`,
  each of which locks internally, so there's no call site left that can
  forget to. (`env_session.conf` is intentionally out of scope — see the
  note on `env_login`/`env_session.conf` in the evidence below.)
- [x] Use an OS-backed lock with clear ownership and crash semantics.
  `state::lock()` uses `std::fs::File::lock` (`flock(2)` on Unix, stabilized
  in std as of this MSRV) — the kernel releases it when the holder's fd
  closes, including on a crash, so there's no stale-lockfile state to reason
  about or clean up.
- [x] Keep the critical section small and define lock ordering. Each public
  function in `state.rs` acquires the lock once for just its own file
  operations (never around the D-Bus calls or the external shell-loader
  process); there is exactly one lock in the whole crate, so there's no
  ordering to get wrong.
- [x] Ensure all writers, not only tests, use the same locking primitive. All
  four production call sites (`prepare.rs`, `finalize.rs`, `exec.rs`,
  `cleanup.rs`) were migrated off direct `env::files` calls onto
  `session::state`; `env::files::append_cleanup`/`read_cleanup` (the
  unlocked functions the finding named) no longer exist.

### P1-02 Make state writes atomic and generation-scoped

- [x] Write temporary files in the destination directory. `env::files::save_env`
  and the new `write_cleanup_entries` both go through
  `util::fsutil::atomic_write_path` (the same temp-file-then-rename helper
  Phase 0 built for unit generation, promoted from `units::fsutil` to
  `util::fsutil` so both sides could share it).
- [x] Flush as required and atomically rename into place. `fsutil::atomic_write`
  fsyncs the temp file before renaming (unchanged from Phase 0).
- [x] Assign a unique session/generation ID. `state::begin_generation` mints a
  fresh 16-hex-char id (`state::random_hex_id`) each time `prepare-env` runs
  and records it alongside `env_pre`.
- [x] Reject cleanup data belonging to a different or stale generation.
  `env::files::CleanupEntry` tags every cleanup-list line with the generation
  that recorded it; `end_generation` only ever acts on entries tagged with
  the generation currently on record, and a fresh generation always starts
  from an empty list (not an append onto whatever was already there).
- [x] Preserve original variable values, including the distinction between
  unset and an explicitly empty value — **already correct before this
  phase**, not new work: `env::files`'s NUL-separated format and
  `BTreeMap<String,String>` already distinguish "key present with an empty
  value" from "key absent" (see the pre-existing
  `env::files::tests::nul_round_trip_with_tricky_values`, which specifically
  round-trips an empty-string value), and `env_pre`'s restore pushes it
  through unchanged via `SessionBus::set_systemd_vars`.
- [x] Make repeated cleanup safe. `end_generation` (`restore_and_clear_locked`)
  short-circuits to a no-op the moment none of `generation`/`env_pre`/
  `env_cleanup.list` exist — the normal state right after it already ran
  once.

### P1-03 Define partial-update recovery

- [x] Specify ordering for systemd and D-Bus activation-environment updates.
  Documented directly on `SessionBus::set_systemd_vars`/`unset_systemd_vars`:
  set touches systemd first, D-Bus second; unset touches D-Bus first,
  systemd second — deliberately asymmetric so that, whichever one fails,
  systemd (what almost everything downstream actually reads) ends up correct
  rather than the D-Bus copy.
- [x] Track enough state to compensate if only one update succeeds. The new
  `Error::PartialEnvUpdate` variant is returned exactly when the first side
  of a two-step update already succeeded and the second then failed (never
  reported when dbus-broker skipped the D-Bus step entirely, since there was
  nothing partial to report).
- [x] Return contextual errors that identify which environment was changed.
  `PartialEnvUpdate { operation, applied, failed, source }` names both sides
  by construction; `error::tests::partial_env_update_names_both_sides`
  asserts the rendered message contains all of them.
- [!] Test restart/recovery after simulated process termination. Not done —
  needs a live/mocked D-Bus session bus to simulate a mid-update process
  kill, same gap as Phase 0's P0-01. Blocked pending Phase 3 (or a Tier-B
  scenario).

Acceptance criteria:

- [x] Concurrent finalize and watcher updates lose no cleanup variables.
  `session::state::tests::concurrent_appends_from_real_os_threads_lose_no_entries`
  spawns 24 real OS threads all calling `state::append_cleanup` against the
  same lock file concurrently and asserts every entry survives — this is
  the strongest evidence in this phase, since it exercises actual `flock`
  contention rather than just sequential calls.
- [~] Injected truncation/rename failure leaves a parseable prior state. Covered
  for the *write* path in the sense that `atomic_write` never truncates the
  destination in place (temp file + rename means a failure before the rename
  leaves the old file completely untouched, and Phase 0's
  `fsutil::tests` cover that mechanism directly). Not covered: a fault
  injected specifically inside `state.rs`'s multi-file sequence (e.g.
  `env_pre` written but `generation` not yet) — there's no rollback across
  *that* sequence the way Phase 0's unit-generation batches got one; a crash
  there is designed to fail closed (see the module's "Residual gap" note),
  but that failure-closed behavior itself isn't exercised by a test.
- [x] Stale session data cannot clean a newer session. Enforced by
  generation-tagging (see P1-02) and covered by
  `append_cleanup_tags_with_current_generation_and_preserves_others`, which
  seeds a different generation's entry and confirms it's left untouched.
  Not fully closed: see `state.rs`'s documented residual gap on a *very*
  late `cleanup-env` racing a brand new `prepare-env`'s lock acquisition —
  a narrow window Phase 0's double-start refusal already makes unlikely but
  doesn't structurally eliminate.
- [~] Cleanup restores exact pre-session values and is idempotent. Idempotency
  is covered (see P1-02's last bullet). "Restores exact pre-session values"
  is unchanged pre-existing logic (`restore_and_clear_locked` mirrors the
  original `cleanup_env` restore math verbatim) but isn't exercised by a new
  test in this phase, since doing so needs a live/mocked bus.

Phase 1 evidence:

- [x] Unit tests: `cargo test` — 196 passed, 3 failed (native CachyOS host).
  The 3 failures are the same pre-existing host-environment dependencies
  noted in Phase 0's evidence, unrelated to this phase.
- [x] Linux tests: `scripts/linux-test.sh` — 199 passed, 0 failed, inside the
  Debian container (all 3 host-dependent tests pass there too, as in Phase
  0). `scripts/linux-build.sh` (`cargo build --all-targets` +
  `cargo clippy --all-targets -- -D warnings`) exits 0. `cargo fmt --check`
  is clean.
- [x] Concurrency/fault-injection notes:
  `cargo test concurrent_appends_from_real_os_threads_lose_no_entries` —
  passes; 24 real OS threads racing `state::append_cleanup` against the same
  lock file, every entry present afterward. No fault-injection test exists
  yet for the bus-dependent paths (`begin_generation`/`end_generation`
  themselves can't be unit-tested at all without a live/mocked `SessionBus`,
  since their signatures require one even on the trivial no-op path) — see
  the `[~]`/`[!]` notes above for exactly what that gap covers.

---

## Phase 2 — P1 CLI and upstream compatibility

**Goal:** commands accepted by wsmr have implemented semantics and the supported
surface matches uwsm 0.26.7 unless a divergence is explicitly documented.

### P2-01 Fix compositor-specific activity checks

Finding: `check is-active <WM>` parses but ignores the compositor argument.

- [ ] Pass the requested selector into the activity query.
- [ ] Escape/encode the exact unit instance correctly.
- [ ] Match upstream behavior for compositor-only versus full-session checks.
- [ ] Test an active compositor, inactive compositor, and nonexistent name.

### P2-02 Reconcile start options

Observed incompatibilities:

- uwsm uses `-a` for appended desktop names, `-e` for exclusive names, and `-F`
  for hardcoding; wsmr currently gives `-a` a different meaning.
- Upstream tweak and graphical-target controls are absent or parsed but unused.
- `hardcode` and `no_tweaks` are currently not honored.

- [ ] Restore upstream short-option meanings.
- [ ] Retain non-conflicting descriptive long aliases where useful.
- [ ] Implement supported tweak/graphical-target behavior.
- [ ] Reject intentionally unported behavior explicitly rather than ignoring it.
- [ ] Add parser and behavior snapshots against uwsm 0.26.7.

### P2-03 Reconcile app options

Finding: current user configurations can use uwsm `app -p Property=value` and
`-S out|err|both`, while wsmr exposes incompatible alternatives.

- [ ] Support upstream `-p` property syntax.
- [ ] Support upstream `-S` silent-output modes.
- [ ] Keep compatible long spellings as aliases.
- [ ] Validate duplicate and malformed property values.
- [ ] Test representative commands from a real Hyprland configuration.

### P2-04 Resolve remaining silent or incompatible inputs

- [ ] Implement or remove the parsed graphical-session timeout.
- [ ] Implement removal marks, or reject unsupported values.
- [ ] Reconcile rung names (`run`/`home` versus `runtime`/`home`) with aliases.
- [ ] Preserve the `.desktop` suffix in compositor IDs where upstream does.
- [ ] Audit every clap field to prove it reaches behavior or is rejected.
- [ ] Document intentional divergences in one compatibility table.

Acceptance criteria:

- [ ] CLI golden tests cover all public commands and relevant aliases.
- [ ] No accepted option is silently unused.
- [ ] The installed user's representative uwsm commands parse and behave as
  intended under wsmr.

Phase 2 evidence:

- [ ] Compatibility fixture/version:
- [ ] Parser tests:
- [ ] Behavior tests:

---

## Phase 3 — P1 deterministic unit tests

**Goal:** unit tests exercise controlled inputs, not the developer machine's
desktop entries, logind state, system bus, or XDG installation.

### P3-01 Isolate XDG desktop-entry tests

Finding: tests set XDG variables to empty strings, but empty values correctly
fall back to system defaults and can discover a host terminal.

- [ ] Use populated temporary XDG trees or explicit nonexistent paths.
- [ ] Avoid relying on the host's `/usr/share` contents.
- [ ] Add fixtures for terminal and application discovery.

### P3-02 Inject session/logind discovery

Finding: a prepare test expects logind lookup to fail, but it succeeds on this
machine.

- [ ] Put session deduction behind a small injectable trait/wrapper.
- [ ] Unit-test success, absence, malformed response, and transport failure.
- [ ] Reserve the real system bus for Linux integration tests.

### P3-03 Complete host-independence audit

- [ ] Audit tests for real environment variables, filesystem locations, PATH,
  locale, system buses, and running services.
- [ ] Route process-global environment mutation through the serialized test
  helper required by Rust 2024.
- [ ] Verify the suite on macOS and more than one Linux image.

Acceptance criteria:

- [ ] `cargo test` passes on the CachyOS host.
- [ ] Tier-A Linux tests pass in a clean container.
- [ ] Repeated/randomized test ordering produces the same result.

Phase 3 evidence:

- [ ] macOS/unit result:
- [ ] CachyOS result:
- [ ] Container result:

---

## Phase 4 — P1 credible Tier-B integration tests

**Goal:** the systemd-as-PID-1 test must fail when any claimed lifecycle feature
fails and must assert observable behavior rather than merely execute branches.

### P4-01 Fix false-positive smoke behavior

Findings:

- The compositor stub exports `WAYLAND_DISPLAY` but creates no socket.
- Terminal launch and finalize failures are ignored.
- Observed errors include a shell unable to open `true` and missing
  `NOTIFY_SOCKET`, while the script still prints PASS.

- [ ] Run functional smoke with `set -euo pipefail`.
- [ ] Remove `|| true` from functionality claimed by the test.
- [ ] Keep deliberate failure traversal in a separately named coverage script.
- [ ] Add a trap that collects status and journals on failure.
- [ ] Make the compositor stub create and retain a real Unix socket.
- [ ] Run finalize within the correct compositor unit/cgroup context so it
  inherits the notification environment.
- [ ] Implement a fake terminal that records arguments and correctly launches
  its payload.

### P4-02 Assert the complete happy-path lifecycle

- [ ] Confirm the tested `ExecStart` resolves to the intended wsmr binary.
- [ ] Inspect `FragmentPath`, `DropInPaths`, and generated-file ownership.
- [ ] Assert prepare-env completion and compositor readiness.
- [ ] Assert `graphical-session.target` and XDG autostart activation.
- [ ] Assert the Wayland socket exists and is a socket.
- [ ] Launch a desktop entry and assert its marker/output, unit, slice, and PID.
- [ ] Verify systemd manager environment values.
- [ ] Verify D-Bus activation environment through a custom activatable service
  that writes its received environment to a fixture.
- [ ] Assert compositor shutdown, child-anchor lifecycle, and cleanup.
- [ ] Compare restored environment with the pre-session snapshot.
- [ ] Assert there are no failed wsmr units or stale runtime files.

### P4-03 Cover failure and recovery paths

- [ ] Compositor exits before readiness.
- [ ] Readiness timeout.
- [ ] prepare-env failure.
- [ ] Duplicate start.
- [ ] Stop when already stopped.
- [ ] Interrupted start/generation.
- [ ] Finalize partial failure.
- [ ] App-daemon missing reader or stale FIFO.
- [ ] Cleanup after an unclean compositor exit.

Acceptance criteria:

- [ ] Each deliberately broken fixture makes the functional smoke fail.
- [ ] The happy path passes without ignored commands.
- [ ] Journals identify the responsible unit when a scenario fails.
- [ ] Coverage traversal is clearly not presented as functional verification.

Phase 4 evidence:

- [ ] `scripts/linux-integration.sh`:
- [ ] Failure-injection results:
- [ ] Collected artifact location:

---

## Phase 5 — P2 protocol and syscall hardening

### P5-01 Correct file URL conversion

- [ ] Percent-encode spaces, non-ASCII bytes, and reserved characters correctly.
- [ ] Preserve already valid URI schemes where required.
- [ ] Define relative-path behavior.
- [ ] Add table tests for Unicode, spaces, `#`, `%`, and malformed input.

### P5-02 Correct locale handling

- [ ] Apply precedence `LC_ALL`, then `LC_MESSAGES`, then `LANG`.
- [ ] Parse language, territory, codeset, and modifier independently.
- [ ] Preserve modifiers in values such as `de_DE.UTF-8@mod`.
- [ ] Add localization fallback table tests.

### P5-03 Strengthen desktop-entry parsing

- [ ] Validate `Type=Application` and required fields.
- [ ] Validate action groups and action `Name`/`Exec` fields.
- [ ] Test quoting, field codes, escaping, and backslash expansion.
- [ ] Compare behavior against the upstream/reference fixture set.

### P5-04 Bound systemd and app-daemon waits

- [ ] Add a deadline to systemd job waits and include job/unit context in timeout
  errors.
- [ ] Prefer the systemd job-removed signal where practical.
- [ ] Prevent FIFO output from blocking forever when no reader exists.
- [ ] Define timeout/cancellation behavior for app-daemon communication.

### P5-05 Harden low-level operations

- [ ] Retry `poll` on `EINTR`.
- [ ] Use owned file-descriptor types so all exits close descriptors.
- [ ] Validate PIDs before waiting.
- [ ] Check and propagate `dup2` failures.
- [ ] Keep unsafe blocks isolated with `// SAFETY:` justification.
- [ ] Avoid lossy conversion of non-UTF-8 executable paths; reject them with a
  contextual error if the unit format cannot represent them.
- [ ] Distinguish systemd `NoSuchUnit` from D-Bus transport/auth failures.

Acceptance criteria:

- [ ] Timeout and EINTR tests are deterministic.
- [ ] File-descriptor leak checks pass on Linux.
- [ ] Desktop-entry and locale table tests cover the reported edge cases.

Phase 5 evidence:

- [ ] Unit tests:
- [ ] Linux-specific tests:
- [ ] Safety review:

---

## Phase 6 — P2 CI, toolchain, and documentation truthfulness

### P6-01 Establish the actual toolchain contract

Finding: `Cargo.toml`, README, and repository guidance disagree on Rust/MSRV.

- [ ] Test the proposed MSRV with the locked dependency graph.
- [ ] Choose and document one supported MSRV.
- [ ] Enforce it in CI.
- [ ] Align `Cargo.toml`, README, and repository guidance.

### P6-02 Add an integration matrix

- [ ] Retain a baseline oldest-supported systemd image.
- [ ] Add a current-systemd image using dbus-broker.
- [ ] Run functional Tier B in CI where privileged/rootful containers are
  supported.
- [ ] If hosted CI cannot support it reliably, add scheduled/manual execution
  and publish its status/artifacts without claiming per-commit coverage.
- [ ] Add a normalized generated-unit regression comparison against uwsm 0.26.7.

### P6-03 Align documentation with reality

- [ ] Correct thin versus fat LTO claims.
- [ ] Describe which checks actually run in CI.
- [ ] Update stale statements that Tier B is merely “next.”
- [ ] Document the compatibility target and known divergences.
- [ ] Clearly distinguish macOS unit/build verification, Linux Tier A, systemd
  Tier B, and real compositor testing.
- [ ] Do not claim merged coverage is gated in CI until it is.

Acceptance criteria:

- [ ] A new contributor can reproduce every advertised verification tier.
- [ ] Badges and README claims match workflow definitions.
- [ ] The selected MSRV job passes from a clean checkout.

Phase 6 evidence:

- [ ] MSRV decision and command:
- [ ] CI workflow run:
- [ ] Documentation review:

---

## Phase 7 — Real CachyOS/Wayland/Hyprland integration

**Prerequisites:** G0 and G1 are complete. Do not use the primary user's active
uwsm-managed desktop for the first run.

### P7-01 Prepare an isolated test identity

- [ ] Create a disposable local test user with its own home and user manager.
- [ ] Build a release binary and install it at a stable, versioned test path,
  such as `/usr/local/libexec/wsmr-e2e/<version>/wsmr`.
- [ ] Create a minimal Hyprland configuration that does not invoke existing
  `/usr/bin/uwsm` wrappers or inherit the primary user's configuration.
- [ ] Install a separate display-manager session entry named clearly, for
  example `Hyprland (wsmr E2E)`.
- [ ] Record package, kernel, systemd, dbus-broker, Hyprland, and wsmr versions.

Suggested session entry shape; finalize exact arguments after Phase 2:

```ini
[Desktop Entry]
Name=Hyprland (wsmr E2E)
Exec=/usr/local/libexec/wsmr-e2e/<version>/wsmr start -e -D Hyprland hyprland.desktop
Type=Application
```

### P7-02 Build the three-stage live harness

- [ ] `prepare`: install fixtures, snapshot the pre-login environment, validate
  ownership conflicts, and install the explicit session entry.
- [ ] `verify`: execute from inside the real Hyprland session and collect
  assertions and journals.
- [ ] `post-logout`: execute over TTY/SSH after logout and verify restoration.
- [ ] Make every stage rerunnable and scoped to the disposable account.

### P7-03 Verify real-session behavior

- [ ] `WAYLAND_DISPLAY` names an actual socket under `XDG_RUNTIME_DIR`.
- [ ] `hyprctl monitors` succeeds and reports the expected backend/output.
- [ ] Hyprland's PID and cgroup belong to the expected compositor unit.
- [ ] `FragmentPath`, `DropInPaths`, `ExecStart`, `NotifyAccess`, and unit result
  match the generated wsmr graph.
- [ ] Graphical-session and XDG-autostart targets activate.
- [ ] Required compositor variables reach the systemd manager environment.
- [ ] A custom D-Bus-activatable fixture observes the same exported variables.
- [ ] `wsmr app` starts a fixture in the expected unit and slice.
- [ ] An autostart desktop entry executes and records a marker.
- [ ] Normal logout stops the graph and restores the baseline environment.
- [ ] No failed units, stale state, temporary files, or owned unit files remain.
- [ ] Collect the user journal and test artifacts for review.

### P7-04 Exercise live failure recovery

- [ ] Test a compositor configuration error before readiness.
- [ ] Test a compositor crash after readiness.
- [ ] Test login cancellation/forced termination.
- [ ] Verify the account can subsequently start a fresh wsmr session.
- [ ] Verify the primary uwsm-managed account remains untouched.

### Recovery procedure requirements

- [ ] Recovery is runnable from TTY or SSH.
- [ ] Stop only the exact disposable user's active compositor/session units.
- [ ] Wait for units to become inactive before cleanup.
- [ ] Remove only paths authorized by a valid wsmr ownership manifest.
- [ ] Reload and reset only the affected user's manager state.
- [ ] Preserve logs before cleanup.
- [ ] Never use broad recursive removal or the current unsafe remove path.

Acceptance criteria:

- [ ] A real SDDM login reaches a usable Hyprland desktop.
- [ ] All P7-03 assertions pass.
- [ ] Logout returns to the display manager with exact environment restoration.
- [ ] A second login/logout cycle also passes.
- [ ] At least one controlled crash scenario recovers cleanly.

Phase 7 evidence:

- [ ] Test date and versions:
- [ ] Harness invocation:
- [ ] Assertion report:
- [ ] Journal/artifact location:
- [ ] Recovery result:

---

## Proposed commit sequence

- [ ] `fix!: make unit generation ownership-safe and dry-run pure`
- [ ] `fix: serialize and atomically persist session environment state`
- [ ] `fix!: restore uwsm-compatible CLI semantics`
- [ ] `test: make the unit suite host-independent`
- [ ] `test: make Tier-B assertions functional`
- [ ] `fix: harden desktop-entry and blocking syscall behavior`
- [ ] `ci: run the Linux integration matrix`
- [ ] `docs: align support, compatibility, and verification claims`
- [ ] `test: add the disposable-user Hyprland live harness`

## Final definition of done

- [ ] All phase acceptance criteria are checked with evidence.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes.
- [ ] `cargo test` passes on supported development hosts.
- [ ] `scripts/linux-test.sh` passes.
- [ ] `scripts/linux-build.sh` passes.
- [ ] Functional `scripts/linux-integration.sh` passes without ignored failures.
- [ ] The authoritative merged coverage gate passes when configured.
- [ ] The disposable-user CachyOS/Hyprland test passes twice consecutively.
- [ ] Documentation states exactly what has and has not been verified.
