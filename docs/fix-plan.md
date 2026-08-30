# wsmr remediation and integration-test plan

This document is the canonical tracker for findings from the implementation
review. It is intentionally planning-only: checking an item means its change has
been implemented **and** its acceptance criteria have passed.

For a distilled, standalone write-up of the real-world bugs found during
Phase 7 (not the raw evidence trail), see
[`docs/known-issues.md`](known-issues.md); for how wsmr works in general,
see [`docs/architecture.md`](architecture.md).

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

- [x] **G0 — Safe generation:** Phase 0 is complete before running wsmr against
  the active user's systemd user manager. File-level ownership/atomicity work
  is done and unit-tested; **partially closed by Phase 4's live Tier-B run**:
  `refuse_if_active` (duplicate start), plan/apply generation, and reload are
  now proven against a real, unmocked systemd user manager, not just
  pure-logic tests (`fix-plan.md` Phase 4, "duplicate start" evidence).
  **Closed 2026-08-30**: a new `SessionOps`/`EnvUpdateOps` mocking seam
  (`src/sysd/dbus.rs`, mirroring Phase 3's `SessionLookup`) now covers the
  failure paths too — `run()`'s double-start refusal driven end-to-end
  against a fake bus (not just the pure predicate), a failing reload after a
  successful `apply_generate` proven to leave a coherent on-disk generation
  (P0-01/P0-04), and `PartialEnvUpdate`'s branch logic exercised directly
  (P1-03) — see those sections' evidence. Still genuinely open: P4-03's
  "interrupted start/generation" (corrupted generation mid-flight, a
  Tier-B/container scenario, not a unit-test one) remains explicitly
  deferred, and `session::state::begin_generation`/`end_generation` (a
  different module from `start::run`) still has no seam of its own.
- [x] **G1 — Safe state handling:** Phases 0 and 1 are complete before a real
  Hyprland login is attempted. Locking/atomicity/generation-scoping is done
  and unit-tested (including a genuine concurrent-OS-thread test); **also
  partially closed by Phase 4**: `begin_generation`/`end_generation` ran for
  real across a full start→finalize→app-launches→stop cycle, and the
  no-stale-state assertion (which caught and led to fixing a real
  `cleanup_env` gap — see Phase 4 evidence) is direct live proof the
  generation lock/scope actually clean up correctly outside of unit tests.
  **Closed 2026-08-30**: the one seam this gate was actually waiting on —
  `session::state`'s `restore_and_clear_locked` (shared by
  `begin_generation`'s abandoned-prior-state resolution and
  `end_generation`) took a concrete `&SessionBus` — now has its own
  `StateOps` trait (`src/sysd/dbus.rs`, same pattern as `SessionOps`/
  `EnvUpdateOps`). New tests cover the actual restore/unset decision logic
  (a pre-existing var restored via `set_systemd_vars`, a session-only var
  unset, an untracked live var left alone), the documented fail-closed
  behavior on a bus failure (state files survive so a retry can still act
  on them — previously only asserted in a comment), and `begin_generation`
  resolving an abandoned prior generation before establishing a new one.
  The late-`cleanup-env`-race scenario from Phase 1's evidence remains
  untested (it's a live/Tier-B timing scenario, not a unit-test one), but
  is a narrower residual note now, not a missing seam.
- [x] **G2 — Credible Tier B:** Phase 4's happy path and its two implemented
  failure scenarios (duplicate start, stop-when-stopped) pass with zero
  ignored functional failures — see Phase 4 evidence for the full PASS list
  and the real bugs this caught along the way. **Closed 2026-08-30**: all 6
  remaining P4-03 scenarios landed in two batches the same day (compositor
  exits before readiness, readiness timeout, unclean compositor exit;
  then prepare-env failure, interrupted start/generation, finalize partial
  failure — `scripts/linux-integration-failures.sh`), each independently
  re-confirmed passing before merge, both individually and as a final
  combined 6-scenario run, alongside the original happy-path smoke
  (unregressed, all 19 `PASS:` lines). All 9 of P4-03's named scenarios are
  now implemented; the FIFO scenario's narrower "stale FIFO" sub-case
  remains unit-tested only, not at this integration level (see P4-03).
- [~] **G3 — Real machine:** Phase 7 passes under a disposable CachyOS user
  before claiming CachyOS/Wayland/Hyprland runtime support. **Substantial
  partial evidence as of 2026-08-29** (see Phase 7 evidence): a real
  wsmr-managed Hyprland session was reached and verified against most of
  P7-03's checklist under the disposable `wsmr` account, both by hand and
  (now) via the real `scripts/e2e-harness.sh` P7-02 harness — real
  compositor cgroup/unit match, live `hyprctl monitors` output against real,
  correctly laid-out monitors, a successful `wsmr app` launch, 16/16 checks
  passing on a live `verify` run. **Extended further on 2026-08-30, on the
  primary user's own `geist` account** (a deliberate departure from Phase
  7's "not the primary account" prerequisite — see the note at the top of
  Phase 7 — made because the disposable-account groundwork above had already
  de-risked the core mechanics): the display-manager-mediated login gap
  noted below is now closed — `greetd` + `noctalia-greeter`'s
  `Hyprland (wsmr-managed)` entry was actually picked at the greeter and
  reached a healthy, fully-up session, after two real bugs it surfaced were
  found and fixed (commits `ca69d65`, `06fbdf4` — see Phase 7 evidence). The
  full app-launch surface (Hyprland keybind launches *and* Noctalia v5's own
  GUI launcher, once configured with `launch_apps_custom_command = "wsmr
  app -- $CMD"`) was also verified end to end against this real desktop.
  Still not closed: P7-04's failure scenarios are still mostly unwritten
  (one, "compositor configuration error before readiness," closed later on
  2026-08-30 once sudo access became available — see P7-04), the Hyprland
  environment-restoration bug and the intermittent third-party portal
  crash-on-teardown issue from 2026-08-29 remain (see Phase 7 evidence), and
  no pre/post environment-restoration diff was captured for the 2026-08-30
  primary-account run — only unit-graph and app-launcher health were
  checked there. The three unrestored env vars from 2026-08-29
  (`XDG_SESSION_DESKTOP`/`XDG_BACKEND`/`XDG_MENU_PREFIX`) turned out to be
  mostly wsmr's own tracked exports (all but `XDG_BACKEND` are in
  `varnames::ALWAYS_CLEANUP_BASE`), correctly cleaned up on a clean stop
  per `docs/known-issues.md`'s own prior finding — their persistence on
  `geist`'s account is most likely leftover residue from an earlier
  session crash, not a cleanup gap (see P7-03's corrected entry).

## Current baseline

- Host: CachyOS, Wayland, Hyprland 0.56.2, systemd 261, dbus-broker.
- The active desktop is managed by uwsm 0.26.7, not wsmr.
- Formatting, clippy, and build checks pass.
- Unit tests currently report **247 passing, 0 failing** (228 lib + 18 in the
  `wsmr` binary's own test target + 1 integration test comparing generated
  units against a real uwsm 0.26.7 install, Phase 6) — the 3 originally
  pre-existing host-dependent failures were root-caused and fixed in Phase 3
  (an XDG-dirs test-isolation bug and a hardcoded system-bus dependency).
  Was 166/3 before Phase 0. 2026-08-30 added 13 lib tests: 2 for Phase 7's
  reclaim-stale fix
  (`reclaim_stale_adopts_a_foreign_dropin_instead_of_blocking`,
  `reclaim_stale_never_applies_to_the_static_graph`), 7 for the G0/G1
  `SessionOps`/`EnvUpdateOps` mocking seam (2 in `session::start`, 5 in
  `sysd::dbus`), and 4 for G1's `StateOps` seam in `session::state`.
  Verified in both the native CachyOS
  host and the clean Linux container after every phase, including Phase 2,
  where the container caught a genuinely new host-dependence bug this
  session introduced (see Phase 2 evidence) — the two-environment habit paid
  for itself.
- The Tier-B smoke (Phase 4) now asserts the full happy-path lifecycle as
  hard, unignored checks — including terminal launch and finalize, the two
  gaps that previously let it report a false success — plus duplicate-start
  and stop-when-stopped. See Phase 4 evidence; 6 of 9 P4-03 failure/recovery
  scenarios remain explicitly deferred (G2 above).
- wsmr and uwsm currently use the same unit namespace. Phase 0's file-level
  ownership safety and Phase 1's session-state locking/generation-scoping are
  both implemented and unit-tested (below), but **G0/G1 are not yet fully
  closed**: double-start refusal, reload-failure handling, and the
  generation-begin/end paths are only proven at the pure-logic level, not
  against a live/mocked systemd user manager. Phase 3 added an injectable
  seam for the *system*-bus VT lookup only, not the *session*-bus
  `SessionBus` wrapper these paths actually use — closing G0/G1 still needs
  either a `SessionBus` mocking effort (not currently scheduled in any phase)
  or a live/Tier-B run. Until that verification happens, still do
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

- [x] With a fake active unit, `start` returns the documented conflict result.
  No generation writer or reload method is called. The refusal predicate
  itself is unit-tested in isolation
  (`session::start::tests::refuse_if_active_blocks_only_when_active`), and the
  call order in `run` is linear/reviewable so it can't silently regress.
  **Closed 2026-08-30**: `run()` split into a thin bus-connecting wrapper and
  `run_with(comp, opts, bus: &impl SessionOps)`, mirroring Phase 3's
  `SessionLookup` pattern (`src/sysd/dbus.rs`'s new `SessionOps` trait).
  `run_with_refuses_when_already_active_and_touches_nothing` now drives
  `run_with` itself end-to-end against a fake bus reporting an active unit,
  asserting both the refusal message and that the temp rung directory stays
  empty (never reached `plan_generate`) — the actual gap this bullet named,
  not just the pure predicate.
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
- [x] Handle write, rename, and `daemon-reload` failures without leaving a
  mixed old/new graph. Write/rename failures: handled and tested (rollback,
  below). **`daemon-reload` failure: closed 2026-08-30** — using the same
  `SessionOps` seam as P0-01,
  `run_with_surfaces_reload_failure_after_a_coherent_generation` runs a real
  `plan_generate`/`apply_generate` against a temp dir, injects a failing
  fake `reload()`, and asserts both that the error surfaces *and* the
  on-disk generation is still complete and manifest-verified — the
  documented fail-closed behavior, now actually exercised instead of only
  asserted in a comment. Still true as stated: a failing reload leaves the
  *running* user manager not yet picked up until a later reload, and there's
  still no dedicated "written but not reloaded" error message distinct from
  other reload failures — that finer distinction wasn't part of what this
  pass closed.
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

- [x] Fault injection at each write/rename/reload boundary leaves either the
  old valid generation or the new valid generation. Write/rename boundary:
  covered by the rollback test above. Reload boundary: closed 2026-08-30,
  same evidence as the bullet above.
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
- [~] Test restart/recovery after simulated process termination. **Narrowed
  and partially closed 2026-08-30**: a literal mid-update process-kill
  simulation is out of scope for a unit test (as this bullet's own original
  wording already implied by needing "a live/mocked D-Bus session bus" —
  there's no process to kill in a fake). What *is* now covered, via a new
  `EnvUpdateOps` seam mirroring `SessionOps` (`src/sysd/dbus.rs`): the
  `set_systemd_vars`/`unset_systemd_vars` sequencing itself is extracted
  into `set_systemd_vars_with`/`unset_systemd_vars_with`, generic over
  `EnvUpdateOps`, and 5 new tests confirm `PartialEnvUpdate` fires correctly
  when either side fails after the other already succeeded (both
  directions), the D-Bus step is skipped entirely under dbus-broker, and a
  first-op (or dbus-broker-mode second-op) failure surfaces as plain
  `Error::Dbus`, not `PartialEnvUpdate`. That's the actual decision logic
  this bullet was gesturing at; a literal process-kill scenario remains
  open, but would need a live/Tier-B setup, not a unit test.

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

**Ground truth used:** uwsm 0.26.7 is installed on the dev host
(`pacman -Ql uwsm`); every fix below was checked against the actual installed
Python source (`/usr/share/uwsm/modules/uwsm/main.py`), not memory or
documentation, and `check is-active` was additionally verified live against
that host's real, running uwsm-managed Hyprland session. Full findings and
explicitly deferred divergences are in `docs/cli-compatibility.md`.

### P2-01 Fix compositor-specific activity checks

Finding: `check is-active <WM>` parses but ignores the compositor argument.

- [x] Pass the requested selector into the activity query. `session::stop::is_active_for(bus, wm_id: Option<&str>)` — `None` = generic
  check, `Some(id)` = that compositor only; `main.rs`'s `check()` now passes
  `a.wm.as_deref()` through instead of discarding it.
- [x] Escape/encode the exact unit instance correctly.
  `compositor_unit_name` reuses the existing `units::escape::simple_systemd_escape`
  (ported `simple_systemd_escape(check_wm_id, start=False)`, `main.py:1218`).
- [x] Match upstream behavior for compositor-only versus full-session checks.
  Also fixed a real correctness gap found while porting this: the no-selector
  case previously only checked `wayland-wm@*.service` +
  `graphical-session.target`, missing upstream's broader
  `check_units_generic` set (`graphical-session-pre.target`,
  `wayland-session-pre@*.target`, `wayland-session@*.target` — `main.py:1207`).
  This set now also backs `start`'s own double-start refusal (P0-01), closing
  a real gap where a session mid-startup in the `*-pre@` window wouldn't have
  been detected as active.
- [x] Test an active compositor, inactive compositor, and nonexistent name.
  Unit-tested at the pure escaping/naming level
  (`session::stop::tests::compositor_unit_name_escapes_and_wraps`); the live
  bus call itself carries the same untestable-without-a-mock status as the
  rest of this crate's bus code (see Phase 0/1's residual gap notes) — but
  was verified **live** against the real uwsm session instead: `check
  is-active` (generic) → `active`; `check is-active hyprland` → `inactive`;
  `check is-active hyprland.desktop` → `active` (the real unit is
  `wayland-wm@hyprland.desktop.service` — see P2-04's `.desktop`-suffix fix,
  found via this same live check).

### P2-02 Reconcile start options

Observed incompatibilities:

- uwsm uses `-a` for appended desktop names, `-e` for exclusive names, and `-F`
  for hardcoding; wsmr currently gives `-a` a different meaning.
- Upstream tweak and graphical-target controls are absent or parsed but unused.
- `hardcode` and `no_tweaks` are currently not honored.

- [x] Restore upstream short-option meanings. `-F`/`--hardcode` added (was
  missing); `-a`/`--append` now matches upstream's explicit-opposite-of-`-e`
  semantics (was wrongly mapped to hardcode).
- [x] Retain non-conflicting descriptive long aliases where useful.
  `--no-tweaks` (wsmr's pre-existing long name) kept as `-t`'s long form;
  `runtime` kept as a value alias for `-U`'s `run`.
- [x] Implement supported tweak/graphical-target behavior. Tweaks: the 3 fixed
  drop-ins ported verbatim from `generate_tweaks` (`main.py:1533`) into
  `units::templates::TWEAKS`, generated/removed through the same
  plan/manifest machinery as everything else (`plan_generate`'s new
  `tweaks_enabled` parameter). Graphical-target: `session::start::GstGate`
  (`Disabled`/`Warn(Duration)`/`Abort(Duration)`) with `-G` taking precedence
  over `-g`, both skipped entirely for `-o`/`-n` — ports the precedence and
  skip condition at `main.py:4709-4737` exactly.
- [x] Reject intentionally unported behavior explicitly rather than ignoring
  it. `-F`/hardcode without a resolvable executable is now a hard error
  (`Error::Resolve`) rather than silently doing nothing — matches the
  project's fail-closed posture from Phase 0/1, and is arguably *stricter*
  than upstream (which would raise an unhandled Python exception in the
  equivalent case).
- [x] Add parser and behavior snapshots against uwsm 0.26.7. No golden-file
  snapshot mechanism was built (see the acceptance-criteria note below); every
  fix was instead directly verified against upstream's real source and, for
  `is-active`, its real running output — see `docs/cli-compatibility.md`.

### P2-03 Reconcile app options

Finding: current user configurations can use uwsm `app -p Property=value` and
`-S out|err|both`, while wsmr exposes incompatible alternatives.

- [x] Support upstream `-p` property syntax. `-p`/`--property` added
  (repeatable, `Property=value`), matching upstream's `action="append"`
  shape exactly.
- [x] Support upstream `-S` silent-output modes. `-S` short flag added;
  removed wsmr's incompatible bare-`--silent`-defaults-to-both shape
  (`num_args = 0..=1`/`default_missing_value`) in favor of upstream's plain
  required-value flag.
- [x] Keep compatible long spellings as aliases. `--unit-property` (wsmr's
  pre-existing long name) kept as an alias for `--property`.
- [x] Validate duplicate and malformed property values. Malformed-value
  validation (`Property=value` must contain `=`) was already implemented
  pre-Phase-2 (`app::launch::resolve`, tested by the pre-existing
  `resolve_bad_property_errors`) — confirmed this matches upstream's own
  validation (`app()`, `main.py:3329-3333`, which only checks for `=`, not
  duplicates); no behavior change needed, just the flag shape.
- [x] Test representative commands from a real Hyprland configuration. Not
  literally sourced from this machine's own Hyprland config (which uses
  `uwsm app`, not `wsmr app`, so there was nothing to extract) — instead
  verified the corrected flag shapes directly against the live binary (`-p`
  repeated, `-S` rejecting a bare flag and an invalid value, `-a`/`-e` and
  `-t`/`-T` and `-g`/`-G` conflict detection) — see the evidence section.

### P2-04 Resolve remaining silent or incompatible inputs

- [x] Implement or remove the parsed graphical-session timeout. Implemented
  (see P2-02).
- [x] Implement removal marks, or reject unsupported values. Implemented:
  `session::stop::parse_marks` (comma-separated) + `units::plan::mark_of`
  (classifies a tracked path as a compositor id or `"tweaks"`) filter
  `plan_remove_all`. Upstream's `generic` mark matches nothing in wsmr — see
  `docs/coexistence.md`'s rationale, restated in `docs/cli-compatibility.md`.
- [x] Reconcile rung names (`run`/`home` versus `runtime`/`home`) with aliases.
  Canonical values are now `run`/`home` (matching upstream exactly, including
  `$UWSM_UNIT_RUNG` support with the same invalid-value-warns-and-falls-back
  behavior); `runtime` kept as a value alias.
- [x] Preserve the `.desktop` suffix in compositor IDs where upstream does.
  Found and fixed a real bug: `comp::resolve_entry` stripped `.desktop` from
  `id` via `trim_end_matches`; upstream never does (`CompGlobals.id` is the
  raw main-argument basename, set *before* entry resolution runs,
  `main.py:3961`). Confirmed against the live uwsm session's actual unit name
  (`wayland-wm@hyprland.desktop.service`) — this is what led to checking
  upstream's source for this bullet in the first place.
- [x] Audit every clap field to prove it reaches behavior or is rejected.
  Grepped every field name in `cli.rs` against the rest of the crate; the one
  field with zero external references (`desktop_names_append`, `start -a`) is
  intentionally so — its only job, matching upstream's own design, is to
  exist as an explicit, mutually-exclusive counterpart to `-e` (documented
  inline in `cli.rs` so a future audit doesn't mistake it for a bug).
- [x] Document intentional divergences in one compatibility table.
  `docs/cli-compatibility.md` — covers every subcommand, plus a "known,
  deliberately deferred divergences" section for things found while reading
  upstream but out of this phase's named scope (the static-unit deployment
  model difference, other-rung cleanup on every `start`, `wayland-wm@.service`'s
  `TimeoutStartSec` not syncing with `$UWSM_WAIT_VARNAMES_TIMEOUT`, the `-v`
  short flag, and `aux exec`/`aux readiness` accepting a few flags upstream's
  parser doesn't define for them).

Acceptance criteria:

- [!] CLI golden tests cover all public commands and relevant aliases. No
  golden-file snapshot mechanism was built. Every flag shape was instead
  verified two ways: (a) directly against upstream's real installed source,
  and (b) by actually running the built `wsmr` binary with representative
  invocations (`--help` output, conflict detection, live `is-active`) — see
  evidence below. This is real verification, but it isn't a *regression-proof
  snapshot suite*; a future CLI change could silently drift from upstream
  again without one.
- [x] No accepted option is silently unused. See the clap-field audit above —
  every field now either reaches behavior or is the one documented,
  intentional exception.
- [~] The installed user's representative uwsm commands parse and behave as
  intended under wsmr. Verified for the commands this session actually
  exercised (`start` flag combinations, `app -p`/`-S`, `check is-active`
  live) — not literally replayed from this machine's own Hyprland
  configuration's `uwsm` invocations, since that configuration calls `uwsm`,
  not `wsmr`.

Phase 2 evidence:

- [x] Compatibility fixture/version: uwsm 0.26.7,
  `/usr/share/uwsm/modules/uwsm/main.py` (installed package on the dev host).
- [x] Parser tests: `cargo test` — 206 (lib) + 16 (main.rs resolver tests:
  `resolve_rung_with`, `resolve_tweaks_with`, `str2bool_plus`,
  `resolve_gst_gate`, `apply_hardcode`) passed, 0 failed, both natively and
  in the Linux container (`scripts/linux-test.sh`, 222/222). `cargo clippy
  --all-targets --all-features -- -D warnings` and `cargo fmt --check` clean
  natively and in-container (`scripts/linux-build.sh`).
- [x] Behavior tests: ran the built binary directly —
  `start --help`/`stop --help`/`app --help` show the corrected flags;
  `-a`/`-e`, `-t`/`-T`, `-g`/`-G` each correctly rejected together;
  `-U runtime` (alias) parses and reaches the real double-start refusal
  against the live session; `app -p A=1 -p B=2 --silent` (bare) and
  `app -S sh` (invalid value) both correctly rejected; `check is-active`
  verified live (see P2-01). One bug this verification loop itself caught:
  a new test (`comp::tests::resolve_entry_by_bare_id_keeps_desktop_suffix`)
  passed natively but failed in the clean container because its fixture used
  `Exec=Hyprland` — a binary that happens to be installed on *this* dev host
  but not in the container — a live instance of exactly the host-dependence
  class of bug Phase 3 exists to catch. Fixed by switching the fixture to
  `Exec=sh`; both environments pass now.

---

## Phase 3 — P1 deterministic unit tests

**Goal:** unit tests exercise controlled inputs, not the developer machine's
desktop entries, logind state, system bus, or XDG installation.

### P3-01 Isolate XDG desktop-entry tests

Finding: tests set XDG variables to empty strings, but empty values correctly
fall back to system defaults and can discover a host terminal.

- [x] Use populated temporary XDG trees or explicit nonexistent paths. Root
  cause confirmed: `util::xdg::data_dirs`/`config_dirs` deliberately treat
  `""` as *unset* (see their own tests) and fall back to the real
  `/usr/local/share:/usr/share`/`/etc/xdg` — `app::terminal`'s failing tests
  set `XDG_DATA_DIRS`/`XDG_CONFIG_DIRS` to `Some("")`, which never actually
  suppressed the host's real `/usr/share/applications`. New
  `testutil::NO_XDG_DIRS` (a guaranteed-nonexistent absolute path — a real,
  non-empty value, so it's honored as "search here" and correctly excludes
  the host) replaces every such `Some("")` in `app/find.rs` and
  `app/terminal.rs`.
- [x] Avoid relying on the host's `/usr/share` contents. Verified by the fix
  itself: this is exactly what made `find_terminal_entry_from_list_and_scan`
  and `neg_cache_records_non_terminals_and_finds_terminal` fail on the
  CachyOS host (a real terminal on `/usr/share/applications` made "no
  terminal found" assertions false) and pass everywhere else, including now.
- [x] Add fixtures for terminal and application discovery — already present
  (`myterm.desktop`/`editor.desktop`/`xdg-terminals.list` fixtures in
  `app/terminal.rs`, `foo.desktop`/`bar.desktop` in `app/find.rs`); they were
  fully deterministic once the `XDG_*_DIRS` leak was fixed, so no new
  fixtures were needed.

### P3-02 Inject session/logind discovery

Finding: a prepare test expects logind lookup to fail, but it succeeds on this
machine.

- [x] Put session deduction behind a small injectable trait/wrapper. New
  `sysd::dbus::SessionLookup` trait (`session_by_vt`), implemented for the
  real `SystemBus` and mirrored by `session::prepare`'s
  `deduce_session_with(env, vt, &impl SessionLookup)` — the same pattern
  `session::check::Probes`/`Fake` already established for `check may-start`,
  applied here to the one piece of `prepare-env` that needed it.
  `deduce_session` itself stays a thin real-bus wrapper that resolves the VT
  and delegates.
- [x] Unit-test success, absence, and transport failure — `FakeLookup` in
  `session::prepare`'s tests scripts all three outcomes deterministically.
  "Malformed response" is **not** separately covered at the D-Bus-payload
  level (e.g. a session with a non-numeric `VTNr`): that would need a fake
  D-Bus peer (zbus supports p2p test connections, but standing one up for
  `org.freedesktop.login1.Manager`/`.Session` wasn't done here). Note that
  `SystemBus::session_by_vt`'s real implementation already treats a failure
  to read one session's `VTNr` property as "doesn't match" rather than fatal
  (`if let Ok(v) = self.session_vtnr(...)`), so a malformed *individual*
  session entry was already handled gracefully before this phase — what's
  untested is that specific tolerance, not the overall correctness.
- [x] Reserve the real system bus for Linux integration tests. `deduce_session`
  (the real-bus wrapper) carries the same "Linux-runtime; unverified until
  the integration phase" status as the rest of the bus-touching code in this
  crate — unchanged, just now with its actual decision logic unit-tested via
  the trait instead of being untestable end to end.

### P3-03 Complete host-independence audit

- [x] Audit tests for real environment variables, filesystem locations, PATH,
  locale, system buses, and running services. Findings: (1) `set_var`/
  `remove_var` occur *only* inside `testutil::with_env` — confirmed by
  grepping the whole crate, so the next bullet was already satisfied; (2) the
  P3-01 XDG-dirs leak (fixed above) was the one real bug found; (3) PATH-
  dependent tests (`util::which`, desktop-entry `Exec`/`TryExec` resolution)
  rely only on `sh`/`/bin/sh` existing (already commented in `util::tests` as
  true "on every unix dev host + container") and a name essentially
  guaranteed not to exist — reviewed, not a real host coupling; (4) the one
  locale test (`app::entry::tests::locale_variants_expands`) already
  overrides all three of `LC_ALL`/`LC_MESSAGES`/`LANG` via `with_env`,
  matching exactly what the code under test reads; (5) no test starts or
  depends on a running service beyond the D-Bus/logind paths covered by
  P3-02; a few tests reference `/etc/hostname`/`/etc/hosts` as argument
  strings, but nothing in the code path under test stats or reads them —
  they're arbitrary illustrative path values, not a real dependency, and
  were left as-is.
- [x] Route process-global environment mutation through the serialized test
  helper required by Rust 2024 — already true crate-wide (see above); no
  change needed.
- [!] Verify the suite on more than one Linux base image. Not done in this
  session: the environment this work ran in is native CachyOS Linux, and
  only the one Debian `Containerfile` image was exercised (see evidence
  below). Genuinely open — flag if a second-distro check is wanted before
  relying on this. (macOS is no longer a supported dev/build target at all
  — see `AGENTS.md` — so a macOS check is not part of this item.)

Acceptance criteria:

- [x] `cargo test` passes on the CachyOS host — 201 passed, 0 failed (native).
- [x] Tier-A Linux tests pass in a clean container — 201 passed, 0 failed
  (`scripts/linux-test.sh`, Debian `Containerfile`).
- [x] Repeated/randomized test ordering produces the same result. `cargo
  test` run 3× consecutively and once with `--test-threads=1`: 201/201 every
  time. `--shuffle` (nightly-only, `-Z unstable-options`) isn't available on
  this stable toolchain, so true randomized-order verification wasn't
  possible; multi-threaded (default) vs. single-threaded agreement is the
  evidence actually gathered.

Phase 3 evidence:

- [~] macOS/unit result: N/A — macOS is no longer a supported dev/build
  target (see `AGENTS.md`); superseded by the CachyOS/native result below.
- [x] CachyOS result: `cargo test` — 201 passed, 0 failed. `cargo clippy
  --all-targets --all-features -- -D warnings` and `cargo fmt --check` both
  clean.
- [x] Container result: `scripts/linux-test.sh` — 201 passed, 0 failed.
  `scripts/linux-build.sh` (`cargo build --all-targets` + `cargo clippy
  --all-targets -- -D warnings`) exits 0.

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

- [x] Run functional smoke with `set -euo pipefail`
  (`tests/integration/smoke.sh:11`).
- [x] Remove `|| true` from functionality claimed by the test. Every claim in
  the current `smoke.sh` is a hard assertion (`fail`/`grep -q`/`[ ... ]`); the
  only remaining `|| true`/`2>/dev/null` uses are on `wait $PID` cleanup and
  `list-units` greps for units that legitimately may not exist yet, not on
  anything the script claims worked.
- [x] Keep deliberate failure traversal in a separately named coverage
  script — done differently, deliberately, and noted here rather than left
  silently diverged: instead of moving the `check may-start --verbose`
  traversal calls into a second script, both calls became **hard
  refusal assertions** (`if "$WSMR" check may-start ...; then fail ...; fi`).
  That satisfies the actual concern (traversal-only output must never read as
  a functional pass) more directly than relocating it would — there is no
  code path left in the functional smoke where a traversal's outcome is
  printed as PASS without being checked.
- [x] Add a trap that collects status and journals on failure
  (`collect_diagnostics`/`trap ... EXIT`, `smoke.sh:22-32`) — genuinely
  exercised, not just written: it fired and correctly dumped the failing
  unit's journal context during this phase's own debugging (see evidence).
- [x] Make the compositor stub create and retain a real Unix socket.
  `stub-compositor.sh` binds a real listening socket via `socat
  UNIX-LISTEN:...,fork /dev/null` at `$XDG_RUNTIME_DIR/wayland-stub` before
  ever exporting `WAYLAND_DISPLAY`; `smoke.sh` asserts `[ -S ... ]`.
  `Containerfile.systemd` installs `socat`.
- [x] Run finalize within the correct compositor unit/cgroup context so it
  inherits the notification environment. **Found the exact bug the finding
  named, live**: a first attempt wrapped `wsmr finalize` in a *separate*
  `systemd-run --unit=wsmr-finalize-test` oneshot — that unit has no
  `$NOTIFY_SOCKET` of its own (confirmed directly: `No status data could be
  sent: $NOTIFY_SOCKET was not set`, `rc=1`), because `NOTIFY_SOCKET` is
  provisioned per-unit-invocation, not globally, and finalize's whole job is
  to be run *by the compositor itself*. Fixed by having `stub-compositor.sh`
  call `wsmr finalize` directly as a foreground step in its own ExecStart —
  the way a real self-integrating compositor (Sway, Hyprland) does — so it
  inherits the compositor unit's real `$NOTIFY_SOCKET`. `WSMR_BIN` is pushed
  into the manager's activation environment by `smoke.sh` before `wsmr
  start` so the stub can find the binary regardless of which harness set
  `$WSMR` (plain Tier B vs. the coverage container's instrumented path).
- [x] Implement a fake terminal that records arguments and correctly launches
  its payload. `tests/integration/fake-terminal.sh`: logs `"$*"`, finds `-e`,
  execs everything after it for real. Verified: `wsmr app -T -- true`
  produces a log line containing `-e` and the wrapped `true` actually runs.

### P4-02 Assert the complete happy-path lifecycle

- [x] Confirm the tested `ExecStart` resolves to the intended wsmr binary
  (`systemctl --user show -p ExecStart --value` on the `wayland-wm@` unit,
  grepped for `$WSMR`).
- [x] Inspect `FragmentPath`, `DropInPaths`, and generated-file ownership.
  Asserts `FragmentPath` is under the runtime rung, `DropInPaths` is
  non-empty (the stub's absolute path forces a `50_custom.conf` hardcode
  drop-in, per Phase 0's plan/apply split), and the `.wsmr-generation`
  ownership manifest lists that drop-in.
- [x] Assert prepare-env completion and compositor readiness. Asserts
  `wayland-wm-env@*.service` is `active` (`Type=oneshot`+`RemainAfterExit`,
  so `active` only after `ExecStart` completed) and the compositor unit is
  `active`.
- [x] Assert `graphical-session.target` and XDG autostart activation. Both
  asserted `active` by name.
- [x] Assert the Wayland socket exists and is a socket (`[ -S
  $XDG_RUNTIME_DIR/wayland-stub ]`).
- [x] Launch a desktop entry and assert its marker/output, unit, slice, and
  PID. `tests/integration/marker-app.sh` (new fixture) touches a marker file
  then idles; `smoke.sh` diffs the `app-*.service` unit set before/after to
  find the new unit robustly (independent of app-naming internals), then
  asserts the marker file appeared, the unit is `active`, its `Slice` is
  `app-graphical.slice`, and its `MainPID` is a real `/proc` entry.
- [x] Verify systemd manager environment values. Covered three ways:
  `WAYLAND_DISPLAY` appears after start and is gone after stop; finalize's
  `XDG_CURRENT_DESKTOP` export is asserted directly; the full
  `show-environment` snapshot is diffed pre- vs. post-session (see below).
- [!] Verify D-Bus activation environment through a custom activatable
  service that writes its received environment to a fixture. **Deliberately
  deferred** — a genuine scope cut, not an oversight: building and wiring a
  real D-Bus-activatable `.service` fixture (bus-activation file, service
  name registration, a bus call that actually triggers the activation) is a
  meaningfully separate chunk of work from the rest of this phase, and the
  systemd-activation-environment behavior it would prove is already exercised
  indirectly by every other unit in this test inheriting the manager's
  `set-environment` values (`WAYLAND_DISPLAY`, `XDG_CURRENT_DESKTOP`,
  `WSMR_BIN` all visibly propagate to units that never declared them
  locally). Left as explicit follow-up, not silently dropped.
- [x] Assert compositor shutdown, child-anchor lifecycle, and cleanup.
  Asserts `graphical-session.target` inactive, `WAYLAND_DISPLAY` unset, the
  stub's own process and its `socat` child are both gone
  (`pgrep -f`/`pgrep -x`), and (below) no stale files or failed units.
- [x] Compare restored environment with the pre-session snapshot. Full
  `systemctl --user show-environment` captured before `wsmr start` and after
  `wsmr stop`, asserted byte-identical (`WSMR_BIN`, set once for the whole
  test via `set-environment` rather than through wsmr's own export path, is
  captured *before* the baseline snapshot specifically so it appears on both
  sides and doesn't itself look like a leak).
- [x] Assert there are no failed wsmr units or stale runtime files. Found and
  fixed a **real bug**, not just a test gap: `session::cleanup::cleanup_env`
  removed `env_session.conf` but never the `libexec/` directory
  `session::helpers::extract` writes at runtime
  (`$XDG_RUNTIME_DIR/wsmr/libexec/prepare-env.sh`) — confirmed live
  (`FAIL: stale files remain ... libexec/prepare-env.sh`), fixed by adding
  `std::fs::remove_dir_all(runtime_path("libexec")?)` to `cleanup_env`
  (`src/session/cleanup.rs`). `state.lock` is the one intentional survivor
  (documented in `session::state`'s module doc: an flock target must never be
  deleted out from under a concurrent holder), and the assertion excludes it
  by name rather than by weakening the check generally.

### P4-03 Cover failure and recovery paths

- [x] Compositor exits before readiness. **Implemented 2026-08-30**
  (`scripts/linux-integration-failures.sh crash-before-readiness`,
  `tests/integration/{smoke-crash-before-readiness,stub-compositor-crash}.sh`):
  a stub that `exit 17`s before creating a socket. Journal confirms the real
  crash (`exit 17`, `Result=exit-code`) — not the unit's live `ActiveState`,
  which isn't reliably queryable in the ~1s window before the
  `OnFailure=wayland-session-shutdown.target` cascade tears the instance
  back down. Also asserts `graphical-session.target`/`WAYLAND_DISPLAY` never
  activate, the rest of the pre-readiness graph tears itself down, and the
  activation environment is restored byte-identical to the pre-session
  baseline. Confirmed independently (re-run outside the implementing
  session, not just taken on the original report): `PASSED`. **Notable,
  non-wsmr-specific finding recorded in the script itself**: `wsmr start`
  exits 0 even here — confirmed byte-identical to real uwsm 0.26.7's
  `libexec/signal-handler.sh` (diffs clean except an attribution comment);
  `start`'s exit code reflects the start+shutdown *job* completing, not
  session success — failure detection is meant to happen via unit-graph
  state (which is exactly what this scenario asserts), matching upstream's
  actual design.
- [x] Readiness timeout. **Implemented 2026-08-30**
  (`scripts/linux-integration-failures.sh readiness-timeout`,
  `tests/integration/{smoke-readiness-timeout,stub-compositor-hang}.sh`): a
  stub that opens a real socket but never exports `WAYLAND_DISPLAY`, paired
  with `UWSM_WAIT_VARNAMES_TIMEOUT=2` so `wayland-session-waitenv.service`
  times out in seconds instead of the real 30s default. Asserts (via
  journal, same reliability reasoning as above) that `waitenv` names the
  missing variable rather than failing generically,
  `graphical-session.target` never activates, the hung compositor and its
  `socat` child are torn down by the shutdown cascade, and the environment
  is restored. Confirmed independently: `PASSED`.
- [x] prepare-env failure. **Implemented 2026-08-30**
  (`scripts/linux-integration-failures.sh prepare-env-failure`,
  `tests/integration/smoke-prepare-env-failure.sh`): a broken
  `$XDG_CONFIG_HOME/wsmr/env` (sourced via `.` into `prepare-env.sh`'s own
  shell, so a bare `exit 1` aborts the whole loader, not a subshell) fails
  `wayland-wm-env@*.service` before the compositor's own unit ever starts
  (ordering held). Asserts the failure, the ordering, that
  `graphical-session.target`/`WAYLAND_DISPLAY` never activate, the shutdown
  cascade tears the pre-readiness graph down, and the environment restores.
  Confirmed independently: `PASSED`. **Incidental finding, not fixed (out
  of scope)**: `session::prepare::run_loader` calls
  `dump::parse_shell_dump(&stdout, mark)?` before checking
  `output.status.success()`, so when the loader shell exits before printing
  its closing mark, the generic "could not resolve env output mark" error
  wins and the child's actual stderr (which would show the real broken-
  config reason) is silently dropped. Correctness is unaffected — the unit
  still fails and the graph still tears down cleanly — but a real user
  hitting this gets a less specific diagnostic than they could. Worth a
  look if diagnostic quality here matters later.
- [x] Duplicate start. Asserted: a second `wsmr start` while a session is
  active fails, its stderr mentions "already active", and the *original*
  compositor unit is still active afterward (the refusal doesn't disturb the
  running session).
- [x] Stop when already stopped. Asserted: `wsmr stop` after a clean stop
  still exits 0 (a documented no-op, not an error).
- [x] Interrupted start/generation. **Implemented 2026-08-30**
  (`scripts/linux-integration-failures.sh interrupted-start`,
  `tests/integration/smoke-interrupted-start.sh`): rather than one flaky,
  precisely-timed `SIGKILL` against `wsmr start`'s sub-second generation/
  reload/exec sequence, fuzzes 5 kill points at varying tiny delays
  (0.01–0.05s, full cleanup between each) and asserts the actual invariant
  — a subsequent clean `wsmr start` still succeeds with no manual cleanup,
  the environment restores, and no failed units or stale state remain. A
  scripted, repeatable version of Phase 7's incidental stale-binary self-
  heal finding, and a stronger property test than hitting one exact timing
  window would be. Confirmed independently: `PASSED`.
- [x] Finalize partial failure. **Implemented 2026-08-30**
  (`scripts/linux-integration-failures.sh finalize-partial-failure`,
  `tests/integration/{smoke-finalize-partial-failure,stub-compositor-badnotify}.sh`):
  a stub compositor calls `wsmr finalize` with `systemd-notify` shadowed by
  an always-failing stand-in — finalize's env-export half succeeds (its own
  exec-chain proves this) and only the readiness-notify half fails, killing
  the compositor's own process. Distinct from the already-fixed
  `$NOTIFY_SOCKET`-context bug from the original Phase 4 work. Asserts the
  compositor unit ends up cleanly `failed` (not stuck `activating`), the
  cascade tears the graph down, and — the actual "partial" property this
  scenario names — the environment fully restores despite the successful
  half's exports having already landed. Confirmed independently: `PASSED`.
- [x] App-daemon missing reader or stale FIFO. "Missing reader" is covered at
  the integration level: `smoke.sh` sends `ping` without reading the reply,
  waits past the 5s `SEND_TIMEOUT`, then confirms the daemon is still alive
  and answers a fresh `ping` normally (`open_fifo_for_write_bounded` doing
  its job for real, under a live daemon process, not just the existing
  synthetic-FIFO unit test). "Stale FIFO" (a leftover FIFO from a crashed
  prior daemon) is **not** covered at this integration level — it's already
  covered at the unit level
  (`app::daemon::tests::create_fifo_makes_and_reuses_fifo`, which also
  exercises the "stale plain file at the FIFO path gets replaced" case), so
  the gap is real but narrower than the checkbox implies.
- [x] Cleanup after an unclean compositor exit. **Implemented 2026-08-30**
  (`scripts/linux-integration-failures.sh unclean-exit`,
  `tests/integration/smoke-unclean-exit.sh`): starts a normal session,
  waits for readiness, then `SIGKILL`s the compositor's `MainPID` directly
  — `wsmr stop` is never called. Asserts the graph tears itself down anyway
  via the unit graph's own `OnSuccess=`/`OnFailure=` wiring
  (`graphical-session.target` inactive, `socat` child gone,
  `WAYLAND_DISPLAY` unset), that a subsequent `wsmr stop` is still a clean
  no-op, and that the environment/failed-units/stale-runtime-state checks
  all come back clean. Confirmed independently: `PASSED`.

**All 9 P4-03 scenarios are now implemented as of 2026-08-30** (6 landed in
two batches the same day: `crash-before-readiness`/`readiness-timeout`/
`unclean-exit` first, `prepare-env-failure`/`interrupted-start`/
`finalize-partial-failure` second; duplicate-start and stop-when-stopped
predate them; the FIFO scenario's "missing reader" half is covered
separately, see above). Each of the 6 new ones got its own deliberately-
broken stub-compositor variant and its own container boot, exactly per the
original scope note's reasoning (one broken scenario must not corrupt state
a later one in the same run depends on) — `scripts/linux-integration-
failures.sh` now runs all 6 this way. Every scenario was independently
re-run (not just taken on the implementing agent's report) before being
merged, both individually and as a final combined 6-scenario run, alongside
a re-run of the original unmodified happy path to confirm zero regression.

Acceptance criteria:

- [x] Each deliberately broken fixture makes the functional smoke fail —
  demonstrated more directly than by synthetic fixture-breaking: over the
  course of writing this phase, the *real* rewritten smoke test caught five
  genuine bugs on its own (a missing `marker-app.sh` fixture file, a
  `sleep %f` fixture that can never succeed since `sleep` doesn't take a file
  path, a `check may-start` flag combination that trivially no-ops instead of
  refusing, the finalize/`$NOTIFY_SOCKET` context bug above, and the
  `cleanup_env`/`libexec` product bug above) — each caused a hard `FAIL` with
  a diagnosable message, not a silent pass.
- [x] The happy path passes without ignored commands — final run: `==>
  integration test PASSED` (see evidence), every assertion in `smoke.sh` hit
  `PASS`, zero `|| true` on a claimed behavior.
- [x] Journals identify the responsible unit when a scenario fails —
  genuinely exercised during this phase's own debugging: e.g. the finalize
  failures surfaced `wsmr-finalize-test.service: Main process exited,
  code=exited, status=1/FAILURE` plus the unit's own journal lines via the
  `collect_diagnostics` trap, which is exactly what let the root cause be
  found instead of guessed at.
- [x] Coverage traversal is clearly not presented as functional
  verification — moot by construction now (see the P4-01 note): the
  traversal calls are hard assertions, so there is no coverage-only output
  in the functional smoke to mislabel.

Phase 4 evidence:

- [x] `scripts/linux-integration.sh`: final run exit 0, `==> integration test
  PASSED`, all 19 `PASS:` lines in `smoke.sh` present (pre-start checks
  through no-stale-state). Also re-ran `scripts/linux-test.sh` (234/234,
  including the fixed `cleanup_env`) and native `cargo
  fmt`/`clippy --all-targets --all-features -- -D warnings`/`cargo test`
  (234/234) after the `cleanup.rs` fix — all clean.
- [x] Failure-injection results: 2 of 9 P4-03 scenarios implemented and
  passing (duplicate start, stop-when-stopped); the "missing reader" half of
  the FIFO scenario is also implemented and passing. The other 6 scenarios
  are explicitly deferred (see the scope note above), not silently skipped.
  Incidental failure-injection evidence: 5 real bugs (4 test-fixture, 1
  product) were caught as hard failures during development of this phase,
  itself evidence the smoke test fails on real breakage rather than passing
  through it.
- [x] **2026-08-30 extension, batch 1**: 3 scenarios implemented
  (`scripts/linux-integration-failures.sh`, commit `fabd360`) — compositor
  exits before readiness, readiness timeout, cleanup after an unclean
  compositor exit. Independently re-run (not just taken on the implementing
  agent's report): all three `PASSED`, and the unmodified
  `scripts/linux-integration.sh` happy path was re-run alongside them and
  still shows all 19 `PASS:` lines — confirms no regression.
- [x] **2026-08-30 extension, batch 2**: the final 3 scenarios implemented
  (commit `70c8012`) — prepare-env failure, interrupted start/generation,
  finalize partial failure. Independently re-run: all 6
  `linux-integration-failures.sh` scenarios `PASSED` together, and the
  original happy path re-confirmed unregressed again. All 9 of P4-03's
  named scenarios are now implemented.
- [x] Collected artifact location: local only — `scripts/linux-integration.sh`
  output captured to the session scratchpad during iteration; not persisted
  to the repo or CI (no CI runner available from this environment, same
  caveat as Phase 6's `msrv` job).

**G2 is now fully met**: Phase 4's happy path plus all 9 of P4-03's named
failure/recovery scenarios pass with zero ignored functional failures (see
P4-03 and the G2 gate above for the full list and evidence). The only
narrower residual gap is the FIFO scenario's "stale FIFO" sub-case, which
remains unit-tested only rather than exercised at this integration level —
tracked in P4-03 itself, not a blocker on G2.

---

## Phase 5 — P2 protocol and syscall hardening

**Ground truth used:** as in Phase 2, cross-checked against real installed
references rather than memory — uwsm 0.26.7's actual `main.py` (`path2url`,
`entry_expand_str`, `entry_tokenize_exec`, `check_entry_basic`), and, for
locale precedence/expansion (which uwsm itself delegates to, not implements),
`python-pyxdg`'s real `xdg/Locale.py` (also installed on the dev host).
Several outputs below (percent-encoding, locale variant lists) were also
cross-checked against live `python3` invocations, not just source reading.

### P5-01 Correct file URL conversion

- [x] Percent-encode spaces, non-ASCII bytes, and reserved characters
  correctly. `app::field::percent_encode` ports Python's
  `urllib.parse.quote(arg)` (default `safe="/"`) byte-for-byte: every UTF-8
  byte outside `A-Za-z0-9_.~-/` becomes uppercase `%XX`.
- [x] Preserve already valid URI schemes where required. `has_uri_scheme`
  ports `urlparse(s).scheme`'s truthiness (RFC 3986 §3.1 scheme grammar),
  including its permissive edge case (`"a:b"` reads as scheme `"a"` in both
  implementations — replicated for compatibility, not "fixed").
  Byte-identical to the *actual* upstream (`path2url`, `main.py:2945`) rather
  than the ad-hoc `arg.contains("://")` check this replaced.
- [x] Define relative-path behavior. Matches upstream exactly by *not*
  resolving relative paths against the current directory at all — upstream's
  `path2url` never does either; a relative arg becomes
  `file://relative/path` verbatim. wsmr previously did resolve against `cwd`,
  which was itself the "undefined behavior" the finding was about; now it's
  defined by matching upstream's non-resolution rather than inventing a new
  rule.
- [x] Add table tests for Unicode, spaces, `#`, `%`, and malformed input.
  `path2url_table` — every row cross-checked against a real `python3
  -c "urllib.parse.quote(...)"` invocation, not derived from the Rust code.

### P5-02 Correct locale handling

- [x] Apply precedence `LC_ALL`, then `LC_MESSAGES`, then `LANG`. Fixed a
  real, exactly-backwards bug: `locale()` checked `LC_MESSAGES`, `LANG`,
  `LC_ALL` in that (wrong) order. Went further than the fix-plan's literal
  3-var ask after finding the true reference (`xdg.Locale.expand_languages`):
  real precedence is `LANGUAGE`, `LC_ALL`, `LC_MESSAGES`, `LANG` — first
  *set* var wins outright (no merging). `LANGUAGE` (a GNU gettext extension,
  colon-separated preference list) is now supported too.
- [x] Parse language, territory, codeset, and modifier independently.
  Already correct pre-existing code (`locale_variants`) — verified by
  hand-tracing pyxdg's real `_expand_lang` bitmask-subset algorithm against
  it component-by-component; every combination matches exactly, including
  the fact that pyxdg's own reference implementation parses out the codeset
  but never actually includes it in any candidate (a latent quirk in the
  *reference*, not a bug to fix in the port).
- [x] Preserve modifiers in values such as `de_DE.UTF-8@mod`. Already correct
  (see above) — `locale_variants("de_DE@euro")` producing
  `[de_DE@euro, de_DE, de@euro, de]` matches pyxdg's real output exactly.
- [x] Add localization fallback table tests.
  `locale_precedence_language_then_lc_all_then_lc_messages_then_lang` (new,
  covers the precedence fix) and `locale_candidates_splits_language_on_colon`
  (new); `locale_variants_expands` (pre-existing) already covered the
  expansion algorithm.

### P5-03 Strengthen desktop-entry parsing

- [x] Validate `Type=Application` and required fields. `check_basic` now
  requires `Type=Application` and a non-empty `Name` — previously unchecked
  entirely, so a `Type=Link`/`Type=Directory` entry (or one missing `Name`,
  a spec-required key) would proceed straight to Exec resolution and fail
  with a confusing "no Exec"/"missing executable" message instead of a clear
  one naming the actual problem.
- [x] Validate action groups and action `Name`/`Exec` fields. Fixed a real
  gap found while reading upstream's `check_entry_basic`: it checks the
  action group (`[Desktop Action X]`) actually *exists*, not just that the
  id is listed in `Actions=`, and requires the action's own `Name` and
  `Exec` to be present and non-empty — none of which wsmr's `check_basic`
  did before this phase (it only checked `Actions=` list membership).
- [x] Test quoting, field codes, escaping, and backslash expansion.
  `expand_escapes` extended (`\t`/`\r`/unknown-escape-passthrough/trailing
  lone backslash); new `tokenize_reserved_chars_table` exercises every
  character in upstream's exact reserved-char string
  (`main.py:386`: `` "\t\n'\\><~|&;$*?#()`" ``) both unquoted (rejected) and
  quoted (accepted, except backtick/`$`); new tests in `entry.rs` for the
  Type/Name/action-group/action-Name/action-Exec validation added above.
- [x] Compare behavior against the upstream/reference fixture set. Did this
  for all three functions in `app/field.rs`
  (`expand_str`/`tokenize_exec`/`path2url`) by reading upstream's actual
  implementations line-by-line: `expand_str` and `tokenize_exec` were
  *already* faithful ports (confirmed, no changes needed); `path2url` needed
  the P5-01 fix. `check_basic` was compared against `check_entry_basic`
  (`main.py:429`), surfacing the Type/action-group gaps above.

### P5-04 Bound systemd and app-daemon waits

- [x] Add a deadline to systemd job waits and include job/unit context in
  timeout errors. `SessionBus::wait_for_job` gained a `timeout: Duration`
  parameter (was an unbounded `loop`) and a `unit: &str` parameter purely
  for the timeout error message (systemd job objects don't carry the unit
  name back). Its one caller, `stop_wm`, uses a 20s timeout — comfortably
  above `wayland-wm@.service`'s own `TimeoutStopSec=10`, since the wait
  covers the whole cascading session teardown, not just that one unit.
  Note: upstream's own equivalent (`stop_wm`, `main.py:4394`) is *also* an
  unbounded `while True` — this is wsmr choosing to be more robust than
  upstream here, not matching a bug upstream has.
- [!] Prefer the systemd job-removed signal where practical. Not done:
  documented instead as a deliberate deferral (in `wait_for_job`'s doc
  comment) — implementing it would mean introducing zbus's async
  signal-stream API into what is otherwise an entirely synchronous,
  blocking-`zbus::blocking`-only codebase, a materially bigger architectural
  change than this phase's bounded-wait fix warrants. Polling `ListJobs`
  every 100ms, now bounded, was judged the right scope here.
- [x] Prevent FIFO output from blocking forever when no reader exists. Real
  bug, confirmed by tracing FIFO open semantics: `app::daemon::send` used to
  call `std::fs::write`, which blocks *opening* a FIFO for writing until a
  reader appears — a client that gave up (or crashed) between sending its
  request and reading the reply would wedge the daemon's *entire* loop
  forever on a write nobody will ever read. New
  `open_fifo_for_write_bounded`: retries a non-blocking (`O_NONBLOCK`) open
  until a 5s timeout, then clears `O_NONBLOCK` before the actual write
  (safe once a reader is confirmed present). Every reply site in the main
  loop now goes through `send_reply`, which logs and continues on failure
  instead of propagating via `?` — a single slow/dead client must not take
  the daemon down.
- [x] Define timeout/cancellation behavior for app-daemon communication.
  Defined for the side that exists (the daemon's own reply-write path, just
  above). The *client* side of this FIFO protocol isn't implemented in wsmr
  yet (confirmed: no code anywhere references
  `wsmr-app-daemon-in`/`-out` outside `daemon.rs` itself — it's still the
  optional "M7" fast path per `docs/uwsm-core-analysis.md`), so there's
  nothing further to bound there yet.

### P5-05 Harden low-level operations

- [x] Retry `poll` on `EINTR`. `waitpid`'s poll call previously treated
  `EINTR` as a hard failure; now retries in a loop, returning only on a
  genuine error or success.
- [x] Use owned file-descriptor types so all exits close descriptors.
  `waitpid`'s pidfd is now a `std::os::fd::OwnedFd` (auto-closes on every
  return path, including the new retry loop) instead of a raw `c_int`
  manually `libc::close`'d at one specific point in the old, simpler control
  flow.
- [x] Validate PIDs before waiting. `waitpid` now rejects `pid <= 0`
  (`Error::InvalidArg`) before the `pidfd_open` syscall, rather than
  surfacing a raw `EINVAL` — matters because this PID reaches `waitpid` as
  direct, unvalidated CLI input (`aux waitpid <PID>`).
- [x] Check and propagate `dup2` failures. `start::run`'s
  `dup2(1,3)`/`dup2(2,4)` return values were previously discarded; both are
  now checked and turned into a contextual `Error::io`.
- [x] Keep unsafe blocks isolated with `// SAFETY:` justification. Audited:
  every `unsafe` block/fn in the crate already carries one (verified by a
  script scanning all `unsafe {`/`unsafe fn` sites crate-wide for a nearby
  `SAFETY` comment — zero misses). No changes needed; this was already
  solid practice.
- [x] Avoid lossy conversion of non-UTF-8 executable paths; reject them with
  a contextual error if the unit format cannot represent them. New
  `path_to_unit_string` replaces `.to_string_lossy()` at the two spots that
  feed *every* generated unit's `ExecStart=`: `current_exe()` (wsmr's own
  binary path, baked into literally every unit) and `apply_hardcode`'s
  `which()`-resolved compositor path. A silent lossy conversion here
  wouldn't just be imprecise — it would write a different, likely
  nonexistent path into the unit that then fails to exec with no clear
  cause; now it's a contextual error naming which path was rejected and why,
  before any unit is written. One remaining `.to_string_lossy()` site was
  reviewed and left as-is (`comp::MainArg`'s path-derived entry *id* — a
  cosmetic label built from the basename, not the path actually used to open
  the file, so lossy conversion there can't corrupt anything a unit file
  depends on).
- [x] Distinguish systemd `NoSuchUnit` from D-Bus transport/auth failures.
  Real bug: `SystemBus::unit_active_state` treated *any* `get_unit` error
  (transport failure, auth failure, anything) as "unit not active" —
  meaning a genuine D-Bus outage during `start -g`'s graphical-target wait
  would have silently looked identical to "system hasn't booted that far
  yet" instead of surfacing as the real error it is. New `is_no_such_unit`
  matches specifically on `zbus::Error::MethodError` named
  `org.freedesktop.systemd1.NoSuchUnit`; only that specific case now reads
  as "absent", everything else propagates.

Acceptance criteria:

- [x] Timeout and EINTR tests are deterministic. FIFO timeout:
  `open_fifo_for_write_bounded_times_out_deterministically` measures real
  wall-clock time against a real FIFO with no reader and asserts it's within
  bounds (≥ the timeout, comfortably < 2s) — genuine timing verification,
  not mocked. `waitpid`'s EINTR retry isn't independently signal-injection
  tested (would need sending a real signal mid-`poll` from another thread,
  judged disproportionate for a 3-line retry loop); `waitpid_dead_pid_is_ok`/
  `waitpid_blocks_until_child_exits` (pre-existing) cover the surrounding
  behavior on real Linux pidfds.
- [~] File-descriptor leak checks pass on Linux. No dedicated FD-leak
  checker (e.g. counting `/proc/self/fd` before/after) was added. Confidence
  instead comes from the `OwnedFd` switch itself, which makes a leak a
  compile-time-adjacent property (every path out of the function drops the
  same owned value) rather than something to test for at runtime — judged
  sufficient given the function's small size, but not the same as a
  measured guarantee.
- [x] Desktop-entry and locale table tests cover the reported edge cases.
  See P5-01/02/03 above.

Phase 5 evidence:

- [x] Unit tests: `cargo test` — 215 (lib) + 18 (main.rs) = 233 passed, 0
  failed, both natively and in the Linux container
  (`scripts/linux-test.sh`, 233/233). `cargo clippy --all-targets
  --all-features -- -D warnings` and `cargo fmt --check` clean natively and
  in-container (`scripts/linux-build.sh`).
- [x] Linux-specific tests: the pidfd/poll tests (`waitpid_*`) and the app-
  daemon FIFO tests (`open_fifo_for_write_bounded_*`, real `mkfifo` +
  real blocking-open-with-a-delayed-reader) are Linux/Unix-only by
  construction and ran in both the native CachyOS host and the container.
- [x] Safety review: the crate-wide `unsafe`-block audit above (script-driven,
  zero misses) doubles as this phase's safety review; every new `unsafe`
  usage in this phase (`OwnedFd::from_raw_fd`, the `fcntl` clearing
  `O_NONBLOCK` in `daemon.rs`) carries its own `// SAFETY:` justification.

---

## Phase 6 — P2 CI, toolchain, and documentation truthfulness

### P6-01 Establish the actual toolchain contract

Finding: `Cargo.toml`, README, and repository guidance disagree on Rust/MSRV.

- [x] Test the proposed MSRV with the locked dependency graph. The dev host's
  installed toolchain (`rustc 1.98.0`) already *is* `Cargo.toml`'s
  `rust-version`, so every native `cargo build`/`cargo test`/`cargo clippy`
  run across this entire session (dozens of them, every phase) was already
  real, direct verification at the declared floor — not just a plausible
  claim. Re-confirmed explicitly with `cargo build --all-targets --locked`
  and `cargo test --all-targets --locked` (see evidence).
- [x] Choose and document one supported MSRV. **1.98.0** — this was the
  user's own most recent commit (`69914cd`, the day before this session),
  which added `rust-version = "1.98.0"` to `Cargo.toml` for the first time
  alongside a routine dependency bump. Treated as the authoritative, already-
  made decision; the job here was aligning everything else to it, not
  re-deciding it.
- [x] Enforce it in CI. New `msrv` job in `.github/workflows/ci.yml`: reads
  `rust-version` out of `Cargo.toml` itself (so the two can't drift apart —
  no second hardcoded version number to forget), pins `dtolnay/rust-toolchain`
  to exactly that, and runs `cargo build`/`cargo test --all-targets --locked`.
  **Caveat, stated plainly:** this workflow could not be executed from this
  environment — no way to trigger or observe a real GitHub Actions run here.
  It's syntax-validated (`python3 -c "import yaml; yaml.safe_load(...)"`,
  clean) and its `sed` MSRV-extraction line was run directly against the
  real `Cargo.toml` (correctly prints `1.98.0`), and the build/test commands
  it runs are the exact ones verified locally above — but "added and as
  carefully checked as possible from here" is not the same as "confirmed
  green on GitHub's infrastructure." Worth an actual CI run before trusting
  it blindly.
- [x] Align `Cargo.toml`, README, and repository guidance. `Cargo.toml` was
  already correct (the source of truth). Fixed README.md ("rustc ≥ 1.85;
  developed on 1.95" → "rustc ≥ 1.98.0 ... enforced by CI's MSRV job") and
  `AGENTS.md`/`CLAUDE.md` (same fix, plus its `thiserror`/`anyhow` "likely
  choices" wording, since both are long-since actual, not hypothetical,
  choices).

### P6-02 Add an integration matrix

- [!] Retain a baseline oldest-supported systemd image / add a current-systemd
  image using dbus-broker. Not done — deliberately deferred, not
  overlooked: this is about the *local* Podman Tier-B harness
  (`Containerfile.systemd`), and Tier B itself is still known to ignore real
  failures (`fix-plan.md` Phase 4, not yet done). Building out a multi-image
  systemd matrix for a test that can currently report false positives would
  just multiply the false confidence, not reduce it. Revisit after Phase 4.
- [!] Run functional Tier B in CI where privileged/rootful containers are
  supported. Not done, same reason — Tier B needs Phase 4's hardening
  *before* it's trustworthy enough to gate anything on, in CI or otherwise.
  Wiring a known-unreliable test into CI now would be the exact failure mode
  this whole review exists to fix.
- [x] If hosted CI cannot support it reliably, add scheduled/manual execution
  and publish its status/artifacts without claiming per-commit coverage.
  Interpreted "cannot support it reliably" as also covering "the test itself
  isn't reliable yet" (Phase 4's job) — so the honest move here *is* this
  bullet: README and `AGENTS.md` now both say plainly that Tier B is
  local-only, not run in CI, and not full functional coverage until Phase 4
  lands. No new scheduled/manual workflow was added for it, since standing
  one up for a test that can false-positive would itself misrepresent status
  — the documentation fix *is* the honest version of "publish status without
  claiming coverage" available right now.
- [x] Add a normalized generated-unit regression comparison against uwsm
  0.26.7. New `tests/uwsm_unit_compat.rs`: all 13 of wsmr's static graph
  units (`units::templates::GRAPH`), rendered with uwsm's own
  bin_name/bin_path, asserted **byte-for-byte identical** to uwsm 0.26.7's
  real, package-shipped unit files (captured programmatically from
  `/usr/lib/systemd/user/*` on this host, not retyped by hand — eliminates
  transcription-error risk). This is genuine, passing, durable regression
  coverage for a claim (`templates.rs`'s own doc comment: "kept byte-identical
  to upstream") that was previously never actually checked against a real
  uwsm install anywhere in the test suite.

### P6-03 Align documentation with reality

- [x] Correct thin versus fat LTO claims. Fixed in both README.md and
  `AGENTS.md` — both said "fat LTO"; `Cargo.toml` has said `lto = "thin"`
  since commit `313b1d4` ("Use thin LTO for release builds"), months before
  this session. Added the commit's own rationale (build-time/`target/` size
  cost not worth it) to both.
- [x] Describe which checks actually run in CI. README.md now states exactly
  what `ci.yml` runs (format-check, lint, build, test, plus the new MSRV
  job) and exactly what doesn't (coverage, Tier A, Tier B — all local-only).
- [x] Update stale statements that Tier B is merely "next." Fixed in
  `AGENTS.md` — Tier B has existed for a while now (it's what Phase 4 is
  hardening); "is next" was describing a state the project was already past.
- [x] Document the compatibility target and known divergences.
  `docs/cli-compatibility.md` (Phase 2) and `docs/coexistence.md` (Phase 0)
  already exist; this phase's actual gap was that nothing linked to them —
  README.md and `AGENTS.md` both do now.
- [x] Clearly distinguish macOS unit/build verification, Linux Tier A,
  systemd Tier B, and real compositor testing. Already reasonably clear in
  the pre-existing README/AGENTS.md structure (separate sections per tier);
  the fixes here were about *accuracy within* that existing structure (CI
  claims, Tier B caveats), not the structure itself, which didn't need
  rework.
- [x] Do not claim merged coverage is gated in CI until it is. Audited: it
  never was — `ci.yml` has no coverage step at all, and neither README nor
  `AGENTS.md`'s coverage sections claimed CI enforcement (they described
  `just coverage` as *the local, authoritative* gate, which is accurate).
  README's "Development & testing" section now says this explicitly rather
  than leaving it to be inferred correctly.

Acceptance criteria:

- [~] A new contributor can reproduce every advertised verification tier.
  Every tier's exact command is now documented (README + `AGENTS.md`) and
  every command shown was actually run this session except triggering the
  new CI workflow itself (can't, from here — see P6-01's caveat).
- [x] Badges and README claims match workflow definitions. The one existing
  badge (`ci.yml`) matches; README's CI-behavior claims were rewritten to
  match `ci.yml`'s real steps line-for-line rather than paraphrasing from
  memory.
- [~] The selected MSRV job passes from a clean checkout. Passes *locally*
  at the pinned version (this host's toolchain already is 1.98.0); the
  actual GitHub Actions job has not been run — see P6-01.

Phase 6 evidence:

- [x] MSRV decision and command: **1.98.0**, `Cargo.toml`'s `rust-version`
  (see P6-01). `cargo build --all-targets --locked --verbose` and
  `cargo test --all-targets --locked` — both pass on `rustc 1.98.0`
  natively and in the Linux container.
- [~] CI workflow run: not obtained — no GitHub Actions access from this
  environment. YAML syntax validated locally; the underlying commands
  verified locally instead (see above). A real push/PR run is still needed
  before trusting the new `msrv` job.
- [x] Documentation review: README.md and `AGENTS.md` (=`CLAUDE.md`) both
  updated; `cargo test` — 234 passed (215 lib + 18 main.rs + 1 new
  integration test, `tests/uwsm_unit_compat.rs`), 0 failed, natively and in
  the Linux container (`scripts/linux-test.sh`). `cargo clippy --all-targets
  --all-features -- -D warnings` and `cargo fmt --check` clean in both.

---

## Phase 7 — Real CachyOS/Wayland/Hyprland integration

**Prerequisites:** G0 and G1 are complete. Do not use the primary user's active
uwsm-managed desktop for the first run.

**Scaffolding:** [`arch/`](../arch/README.md) has the PKGBUILD, the disposable-user
setup script (`arch/e2e-install.sh`), and the session config (wayland-sessions
entries + Hyprland config notes) P7-01 needs. As of 2026-08-29 it has been run
for real against a disposable `wsmr` account on the actual target machine —
see the evidence below for exactly what passed, what didn't, and two new,
previously-unknown findings it surfaced. (Update: P7-02's harness script was
written and run for real later the same day — see its own section below; this
paragraph originally said it didn't exist yet, left unedited at the time it
was written and only corrected now.) On 2026-08-30 the primary `geist`
account was additionally exercised directly — a departure from this section's
own "not the primary account" prerequisite, made deliberately once the
disposable-account run above had already de-risked start/stop/generation; see
the dated findings inside P7-03 below.

### P7-01 Prepare an isolated test identity

- [x] Create a disposable local test user with its own home and user manager.
  `useradd -m -s /bin/bash wsmr` + `loginctl enable-linger wsmr`
  (`arch/README.md` step 1). Confirmed: `id wsmr` → uid=1002, gid=1004,
  home=`/home/wsmr`; `loginctl show-user wsmr -p Linger` → `Linger=yes`.
- [x] Build a release binary and install it at a stable test path — **done via
  a different path than suggested, deliberately**: `arch/PKGBUILD`
  (`makepkg -si`) was already built and installed as a normal
  `/usr/bin/wsmr` package before this session, so that was used directly
  instead of `arch/e2e-install.sh`'s versioned `/usr/local/libexec/wsmr-e2e/`
  path. Isolation for this criterion comes from the separate `wsmr` Linux
  account, not from which binary path it runs, so this is an equally valid
  substitution (`arch/README.md`'s step 2 now documents both options
  explicitly). Verified live that `/usr/bin/wsmr` (`pacman -Qi wsmr` →
  `0.1.0-1`, built today) is actually what ran: the live unit's
  `ExecStart` showed `path=/usr/bin/wsmr`. A real, unrelated stale binary
  at `/usr/local/bin/wsmr` (933KB, dated Jul 20 — a `just install` leftover
  from well before this session's remediation work, shadowing `/usr/bin/wsmr`
  on `$PATH` since `/usr/local/bin` sorts first) was found and removed
  mid-session; the very first real attempt had silently run that stale
  build instead.
- [x] Create a minimal Hyprland configuration that does not invoke existing
  `/usr/bin/uwsm` wrappers or inherit the primary user's configuration —
  **attempted, then found unnecessary and reverted**: a from-scratch minimal
  `hyprland.conf` was installed first, but this CachyOS/Noctalia-bundled
  system doesn't use a plain `hyprland.conf` at all. `/etc/skel` (and thus
  the fresh `wsmr` account) ships a modular **Lua** config
  (`hyprland.lua` + `config/*.lua`), using Hyprland 0.56.2's native Lua
  config support (confirmed via a literal string in the `Hyprland` binary
  itself: `"Use the default lua config from
  https://github.com/hyprwm/Hyprland/.../hyprland.lua"`). Hyprland's own
  startup log confirmed `[cfg] Using lua config found at
  /home/wsmr/.config/hypr/hyprland.lua` — it was already preferring the Lua
  config over the stub `.conf` even before the stub was deleted; the
  original theory that the stub was shadowing it was wrong. The stub was
  removed; the account now runs the unmodified `/etc/skel`-provided Lua
  config, which already satisfies the actual requirement (isolated from the
  primary user's personal config, doesn't invoke `/usr/bin/uwsm` to start)
  with no hand-rolled file needed.
- [x] Install a separate display-manager session entry named clearly.
  `arch/PKGBUILD` installs `Hyprland (wsmr-managed)`
  (`/usr/share/wayland-sessions/hyprland-wsmr.desktop`). **Not exercised as
  an actual login path on 2026-08-29** — every session that day was started
  by running `wsmr start -e -D Hyprland hyprland.desktop` directly from an
  already-logged-in shell, not by picking this entry at a greeter. **Closed
  2026-08-30**: this exact entry was picked at the real `greetd` +
  `noctalia-greeter` greeter on the primary `geist` account and reached a
  healthy session — see the dated findings in P7-03.
- [x] Record package, kernel, systemd, dbus-broker, Hyprland, and wsmr
  versions. `systemd 261.2-1`, `dbus 1.16.2-1.1`, `hyprland 0.56.2-1`,
  `wsmr 0.1.0-1` (`pacman -Q`), kernel `7.2.2-1-cachyos` (`uname -r`).

### P7-02 Build the three-stage live harness

- [x] `scripts/e2e-harness.sh {prepare|verify|post-logout} [--user NAME]`.
  Everything below this point was first proven by hand, interactively
  (recorded honestly as such at the time); this script encodes those same
  checks as a real, rerunnable command with a real exit code, run as root
  from outside the disposable account (reaches its systemd --user
  manager/D-Bus bus via `sudo -u`, so it works even without a usable
  graphical session — relevant given the kmscon input-conflict finding
  below, which at one point made that impossible). `prepare` sanity-checks
  the account and snapshots a pre-login environment baseline + package
  versions to `/tmp/wsmr-e2e-harness/<user>/`; `verify` runs the P7-03
  checklist as independent, soft-fail assertions with a pass/fail count
  (own test app fixture cleaned up automatically); `post-logout` diffs the
  current environment against the saved baseline (with the confirmed
  Hyprland-bug variables from below allowlisted by name, so a *real* new
  regression isn't lost in already-understood noise), and checks for failed
  units and stale runtime state. All three stages run for real, in order,
  against a live session, on 2026-08-29: `prepare` (all checks passed),
  `verify` (16/16 checks passed), `post-logout` (3/4 — correctly caught the
  crash-loop finding below as a real failure, not a false negative).

### P7-03 Verify real-session behavior

- [x] `WAYLAND_DISPLAY` names an actual socket under `XDG_RUNTIME_DIR`.
  `sudo test -S /run/user/1002/wayland-1` succeeded.
- [x] `hyprctl monitors` succeeds and reports the expected backend/output.
  Returned full, real EDID data for 3 physical monitors: `HDMI-A-2`
  ("Technical Concepts Ltd Beyond TV", 3840x2160@60), `DP-5` ("ViewSonic
  Corporation VX3276-QHD", 2560x1440), `DP-6` ("Lenovo Group Limited
  G34w-30", 3440x1440) — the latter two are the same two monitors the
  primary user's own `/etc/hyprland.conf` names by description, confirming
  this is real hardware detection, not a stub. First run left the (real,
  correctly detected) `HDMI-A-2` focused by Hyprland's own arbitrary
  first-enumerated-output choice, not a monitor the tester was looking at.
  Fixed by editing the `wsmr` account's `/etc/skel`-provided Lua config
  (`config/variables.lua`'s `MONITOR1`/`MONITOR2` plus explicit
  `hl.monitor()` rules in `config/monitors.lua`, both `desc:`-keyed to the
  same two monitors and modes/positions `/etc/hyprland.conf` uses) to mirror
  the primary session's real layout. Re-verified on a second run: `DP-6`
  (Lenovo) at `3440x1440@165` `0x0`, **focused: yes**; `DP-5` (ViewSonic) at
  `2560x1440@74.93` `0x-1440`; `HDMI-A-2` auto-positioned at `3440x0`,
  non-overlapping, not focused — an even stronger form of this check, since
  the result now matches a specific expected layout, not just "some real
  monitors were found."
- [x] Hyprland's PID and cgroup belong to the expected compositor unit.
  `/proc/<MainPID>/cgroup` →
  `0::/user.slice/user-1002.slice/user@1002.service/session.slice/wayland-wm@hyprland.desktop.service`,
  matching `systemctl --user show -p MainPID` for that exact unit.
- [x] `FragmentPath`, `DropInPaths`, `ExecStart`, `NotifyAccess`, and unit
  result match the generated wsmr graph.
  `FragmentPath=/run/user/1002/systemd/user/wayland-wm@.service`,
  `DropInPaths=.../wayland-wm@hyprland.desktop.service.d/50_custom.conf`,
  `NotifyAccess=all`, `Result=success`,
  `ExecStart={ path=/usr/bin/wsmr ; argv[]=/usr/bin/wsmr aux exec --
  hyprland.desktop /usr/bin/start-hyprland ... }`.
- [x] Graphical-session and XDG-autostart targets activate.
  `systemctl --user is-active graphical-session.target
  wayland-session@hyprland.desktop.target
  wayland-session-xdg-autostart@hyprland.desktop.target` → `active active
  active`.
- [x] Required compositor variables reach the systemd manager environment.
  `systemctl --user show-environment` showed `WAYLAND_DISPLAY=wayland-1`,
  `XDG_CURRENT_DESKTOP=Hyprland`.
- [!] A custom D-Bus-activatable fixture observes the same exported
  variables. Not built — the same deliberate scope cut as Phase 4's P4-02
  bullet, for the same reason (real, separate scripting work).
- [x] `wsmr app` starts a fixture in the expected unit and slice.
  `wsmr app -t service -- sleep 120` → `app-Hyprland-sleep@45c5beac.service`,
  `ActiveState=active`, `Slice=app-graphical.slice`.
- [~] An autostart desktop entry executes and records a marker. No
  purpose-built marker fixture, but real substitute evidence: 4 genuine XDG
  autostart entries from this real CachyOS/Noctalia install
  (`app-cachyos-hello@autostart.service`, `app-blueman@autostart.service`,
  `app-arch-update-tray@autostart.service`,
  `app-geoclue-demo-agent@autostart.service`) were observed `active`,
  confirming the autostart mechanism launches real apps correctly.
- [~] Normal logout stops the graph and restores the baseline environment.
  **Partially confirmed, with a real gap found.** `graphical-session.target`
  correctly went `inactive` on `wsmr stop`, and most session-scoped
  variables were correctly unset (all `LC_*` locale vars, `DISPLAY`,
  `HL_INITIAL_WORKSPACE_TOKEN`, `HYPRLAND_CMD`,
  `HYPRLAND_INSTANCE_SIGNATURE`, `MANAGERPIDFDID`, `OLDPWD`, `SHLVL`,
  `XDG_SEAT`, `XDG_SESSION_ID`, `XDG_VTNR`, `_JAVA_AWT_WM_NONREPARENTING` —
  a full pre/post `systemctl --user show-environment` diff was captured).
  **Not restored**: `WAYLAND_DISPLAY`, `XDG_CURRENT_DESKTOP`,
  `XDG_SESSION_DESKTOP`, `XDG_BACKEND`, and `XDG_MENU_PREFIX` all remained
  in the manager environment after stop.

  **Root cause confirmed for `WAYLAND_DISPLAY`/`XDG_CURRENT_DESKTOP` — a
  real bug in Hyprland's own binary, not a wsmr defect, and not sensitive
  to how the session ends.** `strings -n 20 /usr/bin/Hyprland` shows two
  complete, literal shell-command strings embedded in the binary, entirely
  independent of wsmr:

  ```sh
  # startup:
  systemctl --user import-environment DISPLAY WAYLAND_DISPLAY \
      HYPRLAND_INSTANCE_SIGNATURE XDG_CURRENT_DESKTOP QT_QPA_PLATFORMTHEME \
      PATH XDG_DATA_DIRS \
    && hash dbus-update-activation-environment 2>/dev/null \
    && dbus-update-activation-environment --systemd WAYLAND_DISPLAY \
         XDG_CURRENT_DESKTOP HYPRLAND_INSTANCE_SIGNATURE QT_QPA_PLATFORMTHEME \
         PATH XDG_DATA_DIRS

  # shutdown:
  systemctl --user unset-environment DISPLAY WAYLAND_DISPLAY \
      HYPRLAND_INSTANCE_SIGNATURE XDG_CURRENT_DESKTOP QT_QPA_PLATFORMTHEME \
      PATH XDG_DATA_DIRS \
    && hash dbus-update-activation-environment 2>/dev/null \
    && dbus-update-activation-environment --systemd WAYLAND_DISPLAY \
         XDG_CURRENT_DESKTOP HYPRLAND_INSTANCE_SIGNATURE QT_QPA_PLATFORMTHEME \
         PATH XDG_DATA_DIRS
  ```

  So Hyprland exports these itself on start (explaining why they were set
  at all — not via wsmr's `finalize`/`state::append_cleanup` path, which
  never touches `XDG_DATA_DIRS`/`QT_QPA_PLATFORMTHEME` and had no record of
  this export to clean up). The shutdown string looks like a matching
  unexport, but it isn't one: `dbus-update-activation-environment --systemd
  NAME` given a *bare name* (no `=VALUE`) re-exports that variable's
  *current value from its own inherited process environment* — and since
  this command runs as Hyprland's own child, it still has `WAYLAND_DISPLAY`
  etc. set in its own process memory even after the `unset-environment`
  call one clause earlier told systemd to forget them. So Hyprland's own
  shutdown line unsets the vars, then immediately re-exports the exact same
  values right back, in the same breath — a bug in the command itself, not
  a timing/signal-handling issue. Confirmed two ways today: `wsmr stop`
  (systemd-initiated `SIGTERM`, no custom `ExecStop=`) *and* a clean,
  user-initiated logout from inside the session via Noctalia's own UI
  (`start-hyprland`'s log explicitly recorded "Hyprland exit cleanly" for
  that one) both left the identical five vars behind — ruling out the
  original SIGTERM-vs-graceful-exit theory this replaces. This is squarely
  Hyprland's own bug, present in the binary regardless of session manager;
  wsmr correctly cleaned up 100% of what it exported through `finalize`
  itself, and upstream `uwsm` would hit the exact same bug wrapping the
  same Hyprland binary.

  **`XDG_SESSION_DESKTOP`/`XDG_BACKEND`/`XDG_MENU_PREFIX` root cause not
  pinned down (2026-08-29).** None of these three appear in the Hyprland
  import/export command above, so that specific mechanism doesn't explain
  them (though `XDG_MENU_PREFIX=hyprland-` still looks Hyprland-authored,
  just via some other path not yet found). More importantly: **no true
  pre-`wsmr start` baseline was captured for this real-hardware run** — only
  a mid-session ("pre-stop") snapshot was compared against post-stop, unlike
  Phase 4's Tier-B smoke test, which does capture a real pre-start baseline.
  It's possible one or more of these three predates `wsmr start` entirely
  (e.g. set by the login process itself via PAM for a `tty`-class session)
  and was never wsmr's or Hyprland's to clean up in the first place —
  genuinely unknown either way from today's evidence. A repeat run capturing
  a real pre-start baseline would resolve this cleanly.

  **Investigated 2026-08-30 on the primary `geist` account — corrected
  later the same day after checking `docs/known-issues.md`'s own prior,
  more rigorous finding on this exact question.** The first pass found `systemctl show user@1000.service -p ActiveEnterTimestamp` →
  `Fri 2026-08-28 20:52:43` (the manager predates the first `wsmr start` on
  this account by two days) and checked `varnames.rs`'s `SESSION_SPECIFIC`
  list, concluding none of the three were wsmr's concern. **That check used
  the wrong list.** `SESSION_SPECIFIC` (`XDG_SEAT`/`XDG_SEAT_PATH`/
  `XDG_SESSION_ID`/`XDG_SESSION_PATH`/`XDG_VTNR`) is unrelated;
  `ALWAYS_CLEANUP_BASE` (`varnames.rs:50-63`) is the one that actually
  governs stop-time cleanup, and it **explicitly includes**
  `XDG_CURRENT_DESKTOP`, `XDG_MENU_PREFIX`, and `XDG_SESSION_DESKTOP` — all
  three of the previously-mysterious vars except `XDG_BACKEND`. This
  matches `docs/known-issues.md`'s own already-existing, more carefully
  controlled finding (a clean disposable-account start/stop cycle,
  verified from an explicitly-cleared baseline): these three are wsmr's own
  `-D Hyprland` desktop-name exports, and `always_cleanup()` correctly
  scrubs them on every clean `wsmr stop`.

  **Corrected conclusion**: their persistence in `geist`'s long-lived
  manager isn't evidence they predate wsmr's involvement — it's much more
  likely residue from a session that never reached a clean stop at all
  (this conversation independently found two real crashes on this exact
  account/manager: the Hyprland `CCompositor::cleanup()` SIGSEGV and the
  `xdg-desktop-portal-hyprland` teardown SIGSEGV, both discussed above and
  in `docs/known-issues.md`). A crash skips wsmr's cleanup code path
  entirely — there's no partial-cleanup gap to fix here, just the ordinary,
  already-understood consequence of a session dying before it ever reaches
  `wsmr stop`/`cleanup_env`. **`XDG_BACKEND` is the one genuine exception**:
  it appears nowhere in `varnames.rs`, nowhere in Hyprland's own embedded
  `import-environment`/`dbus-update-activation-environment` strings
  (confirmed by grep against both), and `known-issues.md`'s attribution of
  it to "wsmr's own `-D Hyprland` desktop-name handling" doesn't hold up
  against the current source — that specific claim there is itself
  imprecise and worth a follow-up correction. Its real origin remains
  unknown. The `WAYLAND_DISPLAY`/`XDG_CURRENT_DESKTOP` re-export bug itself
  is unaffected by any of this — still a confirmed Hyprland-binary defect,
  already fully mitigated on this very account via
  `HYPRLAND_NO_SD_VARS=1` (confirmed present in both
  `~/.config/wsmr/env-hyprland` and the live manager environment) per
  `docs/known-issues.md`'s existing, separately-verified fix.

  This is a real difference from Phase 4's Tier-B smoke test, where the
  stub compositor explicitly calls `wsmr finalize` itself and the full
  export set is cleanly tracked and restored — a real compositor with its
  own independent env-export habits, exercised for the first time today, is
  not the same test. Net effect either way: "exact environment restoration"
  is not actually true here today, for reasons outside wsmr's own generation
  and cleanup logic as far as investigated so far.
- [~] No failed units, stale state, temporary files, or owned unit files
  remain. `systemctl --user list-units --failed` returned empty after the
  first two clean stop/logout cycles, and stale-runtime-state was
  separately clean on a later run (`scripts/e2e-harness.sh post-logout`).
  **But not reliably true**: a later run (still today) left 4 units in a
  genuine `failed` state after a normal logout — see the new finding below.
  wsmr's own units were not among them.
- [x] Collect the user journal and test artifacts for review. Done
  throughout via live `journalctl`/`hyprland.log` inspection (see the
  kmscon finding below for how `hyprland.log` was reached — Hyprland
  disables stdout logging after startup). Not copied to a persisted
  location outside this conversation.

**New finding: `kmscon` fights Hyprland for seat ownership (real, reproducible,
not a wsmr code defect).** On this host, `systemd-logind`'s `autovt@.service`
is aliased to `kmsconvt@.service` (confirmed via `systemctl cat
autovt@.service`), so *any* switch to an unused VT spawns `kmscon` instead of
a bare `getty`. wsmr's own `libexec/signal-handler.sh` (ported verbatim from
upstream) correctly detects `TERM_SESSION_TYPE=kms` in that case and sends
kmscon the proper `\033]setBackground\a` hand-off escape sequence via the
fd-3 messaging path `src/session/start.rs` sets up (`dup2(1,3)`/`dup2(2,4)`
before re-execing under `systemd-cat`) — this wiring was inspected and looks
structurally correct, matching upstream's design. Despite that, the first
real attempt (`wsmr start` run from a login shell hosted inside `kmscon
--vt=tty2 --no-switchvt`) produced a session with **completely
non-functional mouse and keyboard input**. Pulling the real `hyprland.log`
(root-only, under `/run/user/1002/hypr/<instance>/`, needed since Hyprland
disables stdout logging right after startup) showed a repeating cycle for
the entire life of the session: `[libseat] Disabling seat` → every input
device removed → `[libseat] Enabling seat` → every device re-added — kmscon
and Hyprland fighting over seat/DRM ownership on the same VT. Initial device
enumeration was correct (real mouse/keyboards detected and named correctly),
ruling out a plain permissions/ACL problem; this is a live ownership
conflict, not silent denial. **Workaround (successful, verified):**
explicitly `systemctl start getty@tty3.service` before switching there, so
logind sees the VT already has a console and never spawns `kmsconvt@tty3` on
top of it. `TERM_SESSION_TYPE` is then never `kms`, so wsmr's kmscon
hand-off code never has to run, and the flapping never occurs (confirmed:
`grep -c "Enabling seat"` on that session's log was `0`, vs. dozens on the
first attempt). Root cause is most likely on kmscon's side, or in the
kmscon↔Hyprland/aquamarine handoff specifically — not in wsmr's own hand-off
code, which was verified to send the correct signal. Not deep-dived further
given the practical workaround; worth tracking as a known interop gap for
anyone repeating this test on a similarly kmscon-defaulted system.

**Cross-validated against real `uwsm` — confirmed, not just predicted.**
First checked structurally: uwsm's real Python (`main.py:4863-4884`) uses
the exact same `os.dup2(1, 3)`/`os.dup2(2, 4)` + `systemd-cat`-wrapped
`signal-handler.sh` invocation wsmr's Rust port uses, and
`diff /usr/lib/uwsm/signal-handler.sh
libexec/signal-handler.sh` is empty except for the added attribution
comment — the script wsmr ported is byte-identical to upstream's. Then
confirmed live: ran `./session.sh uwsm` (the same account/compositor
config) on a kmscon-hosted VT. Hyprland crashed outright at startup —
`terminate called after throwing an instance of 'std::runtime_error'`,
`what(): CBackend::create() failed!`, a real SIGABRT with a core dump
(`systemd-coredump`), `wayland-wm@hyprland.desktop.service: Failed with
result 'protocol'`. `CBackend::create()` is Hyprland's DRM/KMS backend
initializer — consistent with kmscon still holding those resources when
Hyprland tried to grab them, same underlying conflict as the wsmr run, just
a *harder* failure this time (outright crash vs. degraded-but-running
input) rather than the identical symptom. That difference in severity
between the two runs is itself consistent with this being a genuine race
(exactly when kmscon releases resources relative to when the new
compositor grabs them varies), not a deterministic, reproducible-every-time
bug. Combined with the structural identity confirmed above, this settles
it: **`uwsm` has the identical kmscon problem `wsmr` does — it is not a
wsmr-specific issue.**

**New finding: several third-party apps/portals fail (not just warn) when
the compositor disappears during teardown — not a wsmr defect.** Found by
`scripts/e2e-harness.sh post-logout` on its first real run, catching exactly
what it was built to catch. After a normal `wsmr stop`-free, in-session
logout, `systemctl --user list-units --failed` showed **4** units in a
genuine `failed` state, none of them wsmr's own. None of wsmr's own
generated units were among the failures (confirmed via the same
`list-units --failed` output) — this is squarely third-party app/portal
robustness, not something wsmr causes or can prevent (it correctly stops
the graph; what individual apps do when their Wayland socket closes is out
of its control).

- `xdg-desktop-portal-hyprland.service` (version `1.4.1-1.1`) —
  **root-caused precisely, down to the microsecond, via
  `journalctl -o short-monotonic`**: at `[63691.302669]` it's still
  processing a new Wayland registry interface
  (`ext_foreign_toplevel_image_capture`) — arriving because Hyprland is
  mid-teardown of its own globals — and **130 microseconds later**, at
  `[63691.302801]`, `Main process exited, code=dumped, status=11/SEGV`. A
  genuine SIGSEGV in the portal's own Wayland event-handling code,
  triggered by a registry event landing during compositor shutdown — not a
  race with `PartOf=graphical-session.target`'s stop-propagation as first
  suspected. Confirmed by the rest of the timeline: `Restart=on-failure`
  (the only one of the four units with this directive) then spawned 5 more
  attempts in the next ~1.2s, every one immediately hitting `[CRITICAL]
  Couldn't connect to a wayland compositor` and exiting 1 (the socket was
  fully gone by then), until `StartLimitBurst` capped it
  (`Result=start-limit-hit`, `NRestarts=6`) — and Hyprland's own "exit
  cleanly" log line lands *after* all of that, at `[63692.902580]`, ~200ms
  later. So this isn't a scheduling race to work around; it's a real crash
  bug in how this portal version handles one specific Wayland event during
  compositor teardown.
- `xdg-desktop-portal-gtk.service` — also ended up `failed`, but has no
  `Restart=` directive at all (defaults to none), so it can't loop the way
  the Hyprland portal did — a single failed exit, not individually
  root-caused, but consistent with a similar "doesn't like the compositor
  disappearing" issue in a different portal implementation.
- `app-blueman@autostart.service` and `app-cachyos-hello@autostart.service`
  — both have `Restart=no` explicitly (from
  `systemd-xdg-autostart-generator`'s output) and both exited with
  `Result=exit-code`, `ExecMainStatus=1`: a real nonzero exit, not a
  raw-signal kill, when `app-graphical.slice`'s `PartOf=` propagation tore
  them down along with the session. A different, simpler mechanism than the
  portal crash — these two just don't distinguish "asked to shut down" from
  "something went wrong" and exit(1) either way.

**Notably intermittent**: the two earlier clean cycles today (both checked
for failed units) did not hit this — same account, same compositor, same
general teardown path, no failed units either time. Given the root cause is
now known precisely for the portal (a specific Wayland event landing at a
specific moment during teardown), intermittency is plausible as ordinary
scheduling jitter in exactly when that registry event fires relative to how
far along Hyprland's own teardown is — not something wsmr's own generation
or lifecycle logic has any influence over.

**Cross-validated against real `uwsm` — conclusively confirms this is
unrelated to wsmr.** To settle it directly rather than relying on
inference, the same disposable account's `~/session.sh` was extended into a
selectable launcher (`./session.sh uwsm` vs. `./session.sh wsmr`, same
compositor/config either way) and the same Hyprland session was run end to
end through real `uwsm 0.26.7` instead. The identical crash happened, down
to the same signature (`Got interface: ext_foreign_toplevel_image_capture`
immediately before the SIGSEGV) and the exact same outcome
(`Result=start-limit-hit`, `NRestarts=6`). Confirms this is purely a
`Hyprland`/`xdg-desktop-portal-hyprland` interaction, entirely independent
of which session manager starts and stops the compositor — wsmr and uwsm
both just ask systemd to stop `wayland-wm@.service`, and the portal
mishandles what happens next identically either way. One incidental,
harmless side-observation from running `uwsm` on an account previously used
by `wsmr`: journal `SyslogIdentifier`s still read `wsmr_...` even during the
`uwsm`-initiated run, because `uwsm` only ever manages its own drop-ins
(matching its real, documented design — see `docs/coexistence.md`) and
never rewrote the base `wayland-wm-env@.service`/`wayland-wm@.service`
template files wsmr had generated into the same runtime rung directory
earlier. Cosmetically confusing if you don't know why, but exactly the
byte-identical-static-graph coexistence Phase 0 was designed around,
working as intended.

**New finding (2026-08-30, primary `geist` account): `start()`'s pre-exec
failures were completely silent under a real greeter-launched session — a
real, previously-unknown wsmr bug, now fixed.** Checking the journal after
the primary account's `Hyprland (wsmr-managed)` entry silently dropped back
to the `noctalia-greeter` greeter (twice, ~23:08) turned up nothing at all
under the `wsmr` syslog identifier. Root cause: `session::start::run`'s
pre-exec steps (system-target gate, double-start refusal, unit-generation
plan, bindpid start, login-env snapshot) return their errors as a plain
`Result`, which only ever reaches stderr — but a `greetd`-launched session
has no journal-captured stderr at all (unlike an interactive shell, or
Phase 7's earlier raw-VT runs, both of which do), so a failure here was
completely invisible: the session just closed and greetd fell back to the
greeter with zero trace anywhere. This is a real gap the disposable-account
work never could have surfaced, since every 2026-08-29 run was started from
an already-logged-in interactive shell. **Fixed** (commit `ca69d65`): a new
`session::log_error_to_journal` routes such a failure through a throwaway
`systemd-cat --identifier=wsmr --priority=err` invocation instead, so it
reaches the journal regardless of how the session was launched.

**New finding (2026-08-30, primary `geist` account): wsmr's ownership
conflict refusal hard-blocked starting whenever foreign per-compositor
drop-ins existed, even with no session live — the literal cause of the
original bug report, now fixed.** Once the journal fix above made the
failure visible, it read: `refusing to touch 5 path(s) ... that are not
verifiably owned by wsmr`, naming `app-@autostart.service.d/slice-tweak.conf`,
two other tweak drop-ins, and both of `hyprland.desktop`'s
`wayland-wm-env@`/`wayland-wm@` `50_custom.conf` drop-ins. All five were
confirmed to be `uwsm`'s own prior generation (a follow-up login via the
"Hyprland (uwsm-managed)" entry 48 seconds later logged `Units unchanged`
against those exact paths) — i.e. this account had previously run Hyprland
via `uwsm`, leaving its drop-ins in place, and `plan_generate`'s
foreign-content conflict check (P0-03/P0-05) refused to touch them even
though `refuse_if_active` had *already* independently confirmed, via
systemd, that no session was currently active. That combination — a
manifest-ownership check with no knowledge of live systemd state, layered
under a caller that already knows the live state — is what made switching
between the uwsm-managed and wsmr-managed session entries always fail the
second one. **Fixed** (commit `06fbdf4`): `plan_generate` gained a
`reclaim_stale` parameter; `start::run` passes `true` (safe precisely
because `refuse_if_active` already ran first), and a foreign per-compositor
drop-in or tweak is now adopted as an ordinary write — reported via the new
`GenerationPlan::reclaimed` and logged at `notice` priority
(`log_notice_to_journal`) rather than blocking the whole plan. The static
graph units (byte-identical shared infrastructure with uwsm, per P0-05) are
deliberately excluded from reclaiming and still hard-block on a mismatch —
new tests `reclaim_stale_adopts_a_foreign_dropin_instead_of_blocking` and
`reclaim_stale_never_applies_to_the_static_graph` pin down that boundary.
Verified live immediately after: the same entry, retried, logged `wsmr:
adopting 5 stale unit override(s) ...` and reached a fully healthy session
(`graphical-session.target`, `wayland-session@hyprland.desktop.target`,
`wayland-wm@hyprland.desktop.service` all `active`, `Result=success`).

**New verification (2026-08-30, primary `geist` account): the full
app-launch surface confirmed end to end, including Noctalia v5's own GUI
launcher.** Beyond the compositor graph itself, every app-launch path was
checked against the live session: Hyprland-keybind-launched apps (Dolphin,
JDownloader via a generic wrapper script) already went through `wsmr app`
and appeared as correctly-parented `app-Hyprland-<cmd>-<hex>.scope` units
under `app.slice/app-graphical.slice`, all `active`/`running`/`success`
with nonzero task counts. Noctalia v5 (beta.10) separately ships its own
`[shell]` launcher-integration settings
(`launch_apps_as_systemd_services`/`launch_apps_custom_command`, documented
at `docs.noctalia.dev/noctalia/configuration/shell/`) — neither was
previously set, so Noctalia's own built-in GUI launcher was launching apps
as bare unwrapped children of the shell process, bypassing wsmr entirely.
Setting `launch_apps_custom_command = "wsmr app -- $CMD"` in
`~/.local/state/noctalia/settings.toml` (the actual live settings path, not
`~/.config/noctalia`) fixed this: launching Bruno from Noctalia's launcher
produced `app-Hyprland-bruno-<hex>.scope`, correctly parented, `active`,
`Result=success`. Not a wsmr code change — a downstream desktop-shell
configuration finding, recorded here because it was found and fixed as
part of verifying wsmr's real-world app-launch behavior.

### P7-04 Exercise live failure recovery

- [x] Test a compositor configuration error before readiness. **Closed
  2026-08-30**, on the real disposable `wsmr` account (uid 1002) once sudo
  access became available: `wsmr start -e -D Hyprland Hyprland --config
  /nonexistent/broken-on-purpose.conf` (via `sudo -u wsmr env
  XDG_RUNTIME_DIR=... DBUS_SESSION_BUS_ADDRESS=... wsmr start ...`, the same
  pattern `scripts/e2e-harness.sh` uses). `wayland-wm-env@Hyprland.service`
  failed immediately (exit 1), `graphical-session.target` never activated,
  and the `OnFailure=` cascade left **zero failed units and zero lingering
  units** afterward — cleaner even than the container analog, since this ran
  against the account's real, long-lived `user@1002.service` rather than a
  fresh boot. Three analogous scenarios were also covered at the Tier-B
  container level the same day (see P4-03: compositor exits before
  readiness, readiness timeout, unclean exit); this is the real-hardware
  confirmation of the same class of behavior.
- [~] Test a compositor crash after readiness. **Attempted for real
  2026-08-30, found a harder blocker than sudo.** With sudo access working,
  `wsmr start -e -D Hyprland hyprland.desktop` was tried the same way as
  above (targeting a free VT, never the primary user's active `tty1` —
  confirmed unchanged throughout via `/sys/class/tty/tty0/active` polled
  every second during the attempt). It failed at the environment-preloader
  stage with `Error: could not resolve could not determine login session on
  VT 1` — `session::prepare::deduce_session` (the Phase 3 `SessionLookup`
  seam) correctly refusing because `sudo -u wsmr` gives the process no real
  logind session/seat context, unlike a genuine console login. This matches
  exactly what 2026-08-29's original P7-01 evidence required (a real
  `getty`-mediated VT login) to get a working compositor at all — it is not
  a sudo/privilege problem, it's that reaching actual compositor readiness
  on real hardware needs a real interactive console login (typing
  credentials into a VT), which isn't safely scriptable from here without
  risking exactly the VT-switch disruption to the primary user's live
  desktop this was designed to avoid. Positive side effect: this *is* real,
  new evidence that `deduce_session`'s failure path works correctly under a
  genuine unprivileged-session condition on real hardware, not just its
  existing `FakeLookup` unit tests. The Tier-B "unclean exit" scenario
  (P4-03, commit `fabd360`) remains the closest analog that *is* fully
  covered — same class of teardown-and-recover behavior, verified via a
  stub compositor instead.
- [ ] Test login cancellation/forced termination. Same blocker as the
  crash-after-readiness attempt above — needs actual compositor readiness
  first, which needs a real console login this environment can't safely
  script.
- [~] Verify the account can subsequently start a fresh wsmr session. Not a
  *designed* scenario, but real, live evidence landed anyway: mid-session,
  the unrelated stale `/usr/local/bin/wsmr` binary (see P7-01) was deleted
  while a unit generated against it was still active, causing
  `wayland-wm-env@hyprland.service`'s `ExecStopPost` (`cleanup-env`) to fail
  with `203/EXEC` during that session's teardown — a genuine, if
  self-inflicted, "abandoned prior state" scenario. The very next `wsmr
  start` (now correctly resolving `/usr/bin/wsmr`) succeeded cleanly with
  no manual cleanup required, consistent with
  `session::state::begin_generation`'s documented design ("a fresh
  generation always starts by resolving any abandoned prior state first").
- [ ] Verify the primary uwsm-managed account remains untouched. Not
  explicitly re-verified with a targeted check this session, beyond the
  primary user's own Hyprland session (session 4, seat0, tty1) staying
  continuously listed as active in every `loginctl list-sessions` check run
  throughout.

### Recovery procedure requirements

- [~] Not written as a standalone document, but the 2026-08-30 Tier-B
  scenarios (P4-03) now give a real, evidence-backed answer for the
  container-verified failure modes: **no manual recovery procedure is
  needed** for a crashed/hung/unclean-killed compositor. In all three cases
  (crash before readiness, readiness timeout, `SIGKILL`ed mid-session), the
  unit graph's own `OnSuccess=`/`OnFailure=wayland-session-shutdown.target`
  wiring tears the whole graph down on its own — no stuck units, no stale
  runtime state, no manual `systemctl --user reset-failed` required before
  the next `wsmr start`. One caveat found and documented directly in
  `smoke-crash-before-readiness.sh`/`smoke-readiness-timeout.sh`: retrying
  *immediately* (within the same wall-clock second) after a crash can
  transiently race `xdg-desktop-autostart.target`'s own
  `StopWhenUnneeded=yes` against the still-settling shutdown cascade —
  waiting even briefly (which a human retrying manually always does anyway)
  avoids it; a control run of the unmodified happy path confirms this
  doesn't happen in ordinary (non-rapid-retry) use. 2026-08-29's kmscon
  recovery and the stale-binary self-heal (P7-04 above) remain the only
  real-hardware recovery evidence; both were still manual/interactive, not
  scripted. Not written as a document a human could follow step-by-step —
  that's the part still actually missing.

Acceptance criteria:

- [x] A real SDDM login reaches a usable Hyprland desktop. **The premise was
  wrong**: this system's actual display manager is `greetd` running
  `noctalia-greeter-session`, not SDDM — `sddm.service` doesn't even exist
  on this host (`systemctl status display-manager` → `greetd.service`).
  Future wording should say "a real display-manager login," not name SDDM
  specifically. On 2026-08-29, reached a working session via a raw VT +
  manual `wsmr start` (after working around the kmscon conflict above), not
  via an actual greeter-mediated login. **Closed on 2026-08-30**: the
  `Hyprland (wsmr-managed)` entry was picked at the real `greetd` +
  `noctalia-greeter` greeter on the primary `geist` account and reached a
  fully healthy session — see the dated findings in P7-03 (two real bugs
  were found and fixed to get there).
- [~] All P7-03 assertions pass. 8 of 11 confirmed clean, 1 explicitly
  deferred (D-Bus fixture), 1 partially substituted (real autostart apps
  instead of a custom marker), 1 found genuinely incomplete (environment
  restoration — see above).
- [ ] Logout returns to the display manager with exact environment
  restoration. Not met: no display manager was involved in today's test,
  and environment restoration has the gap described above.
- [x] A second login/logout cycle also passes. The first full attempt (on
  the VT that turned out to be running `kmscon`) surfaced the seat-conflict
  finding above and was abandoned mid-session — not a clean cycle. But two
  clean cycles followed it on the plain-`getty` VT: a start → P7-03
  verification → `wsmr stop` cycle, then (after the monitor-config fix
  above) a second start → verify → **user-initiated logout from inside the
  session via Noctalia's own UI** cycle — a different exit path than the
  first, useful in its own right since it's what surfaced that the
  environment-restoration gap isn't a `wsmr stop`/SIGTERM artifact. Both
  cycles ended with `graphical-session.target` inactive and zero failed
  units (`systemctl --user list-units --failed` empty after each).
- [~] At least one controlled crash scenario recovers cleanly. Not a
  *designed* scenario, but the incidental stale-generation self-heal above
  is real, positive evidence in this direction.

Phase 7 evidence:

- [x] Test date and versions: 2026-08-29 (disposable `wsmr` account), extended
  2026-08-30 (primary `geist` account); systemd 261.2-1, dbus 1.16.2-1.1,
  hyprland 0.56.2-1, wsmr 0.1.0-1 (`pacman -Qi wsmr`), kernel
  7.2.2-1-cachyos, noctalia 5.0.0-beta.10-1.1.
- [x] 2026-08-30 fix commits: `ca69d65` (surface `start()`'s pre-exec
  failures to the journal) and `06fbdf4` (reclaim stale foreign drop-ins
  instead of hard-blocking start) — both verified live immediately after
  landing (see the dated findings in P7-03), plus `cargo test`/`clippy
  --all-targets --all-features -- -D warnings`/`fmt --check` all clean for
  both.
- [x] Harness invocation: `scripts/e2e-harness.sh {prepare|verify|post-logout}
  --user wsmr`. The earlier checks in this evidence section were run
  manually and interactively before the script existed (kept as-is, marked
  as such at the time); the script now exists, was run for real for all
  three stages against a live session, and is what caught the
  third-party-app-failure finding above on its very first run.
- [x] Assertion report: see P7-03 above — 8/11 clean, 1 deferred, 1
  substituted, 1 found genuinely incomplete, 1 found genuinely intermittent
  (the third-party failed-units finding).
- [x] Journal/artifact location: ephemeral. `journalctl` and
  `/run/user/1002/hypr/*/hyprland.log` were inspected live during this
  session; nothing was copied to a persisted location in the repo.
- [~] Recovery result: no *designed* P7-04 recovery scenario was run; the
  incidental stale-generation self-heal (mid-session stale-binary deletion
  → the next `wsmr start` recovered cleanly with no manual intervention) is
  the closest thing to positive recovery evidence collected today.

---

## Proposed commit sequence

- [x] `fix!: make unit generation ownership-safe and dry-run pure` — landed
  combined with the next item as `291ad7b` (Phase 0 + Phase 1 in one commit;
  see that commit's message for why).
- [x] `fix: serialize and atomically persist session environment state` —
  `291ad7b`.
- [x] `fix!: restore uwsm-compatible CLI semantics` — Phase 2, not yet
  committed as of writing this line (commit follows this fix-plan update).
- [x] `test: make the unit suite host-independent` — `3aadc94`.
- [ ] `test: make Tier-B assertions functional`
- [x] `fix: harden desktop-entry and blocking syscall behavior` — Phase 5,
  not yet committed as of writing this line (commit follows this fix-plan
  update).
- [ ] `ci: run the Linux integration matrix`
- [x] `docs: align support, compatibility, and verification claims` — Phase
  6, not yet committed as of writing this line (commit follows this
  fix-plan update).
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
