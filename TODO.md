# TODO

What's actually left to verify, as of 2026-08-30. This is not a design
document — see [`docs/architecture.md`](docs/architecture.md) for how wsmr
works, and [`docs/known-issues.md`](docs/known-issues.md) for real-world
findings already root-caused. This file is just the open list.

wsmr's core is feature-complete (the full `uwsm` core CLI surface is ported
and behavior-matched against real upstream source) and the automated test
suite is green end to end: 247 unit tests, all 9 Tier-B failure/recovery
scenarios, 92%+ merged line coverage, and CI (`ci.yml`) runs all of it —
including the systemd-as-PID-1 integration suite — on every push. What
remains is real-hardware verification that specifically needs a live human
at a console, plus a handful of smaller, self-contained gaps.

## Needs a real interactive console login (not remotely scriptable)

Two of P7-04's real-hardware failure scenarios need an actual compositor to
reach readiness on the disposable test account, which needs a genuine
`getty`-mediated VT login (typing credentials at a physical/virtual
console) — confirmed this can't be safely scripted from an automated
session without risking a live desktop's display. Whoever has physical
access to the test machine needs to:

- [ ] **Compositor crash after readiness.** Log into the disposable `wsmr`
  account for real, get to a running Hyprland session, then kill it (e.g.
  `kill -9` the compositor's PID) and confirm the graph tears itself down
  and the environment restores — the real-hardware counterpart to the
  already-passing containerized `unclean-exit` Tier-B scenario.
- [ ] **Login cancellation / forced termination.** Same login requirement;
  simulate a display-manager-side forced session termination mid-login and
  confirm clean teardown.

(The third P7-04 scenario, compositor configuration error before readiness,
**is done** — verified 2026-08-30 without needing a console login, since a
broken config fails before the compositor ever touches the display.)

## Self-contained follow-up work (no special access needed)

- [ ] **Write an actual recovery-procedure doc.** The evidence that
  self-recovery works is solid (three Tier-B scenarios plus the real-hardware
  config-error case all confirm no manual intervention is ever needed after
  a crash), but nobody's written the operator-facing "if X happens, do Y"
  page. Should be short — the honest answer for every case tested so far is
  "do nothing, `wsmr start` again."
- [ ] **A custom D-Bus-activatable fixture** proving activation-environment
  propagation to a service that never declared the variables itself.
  Deliberately deferred twice (Tier-B and real-hardware) as a distinct chunk
  of scripting work — building and registering a real bus-activation `.service`
  fixture is more involved than everything else in those test passes.
- [ ] **CLI golden-snapshot tests.** Every flag was verified by hand against
  real upstream `uwsm` source and a live binary run, which is real evidence
  but not a regression-proof suite — a future CLI change could silently
  drift from upstream again with nothing to catch it.
- [ ] **The FIFO "stale FIFO from a crashed daemon" scenario** is unit-tested
  (`app::daemon::tests::create_fifo_makes_and_reuses_fifo`) but not exercised
  at the Tier-B integration level, unlike its "missing reader" sibling.
- [ ] **`XDG_BACKEND`'s origin is still genuinely unknown.** Confirmed it's
  not in wsmr's own `varnames.rs`, not in Hyprland's embedded
  `import-environment`/`dbus-update-activation-environment` strings, and not
  from PAM. Harmless (wsmr isn't leaking it, whatever the source), but
  unexplained.
- [ ] **Re-verify the `xdg-desktop-portal-hyprland` SIGSEGV mitigation.** The
  `ExecStop=` drop-in in [`docs/known-issues.md`](docs/known-issues.md) is
  installed on the disposable account but explicitly marked "tried, not
  verified" — the two test cycles that didn't crash aren't strong evidence
  either way, since the bug is already known to be intermittent. Needs
  several more real crash-teardown cycles to say anything conclusive, or
  wait for [hyprwm/Hyprland#15776](https://github.com/hyprwm/Hyprland/pull/15776)
  to land in a release and re-test — that change should make the whole class
  of bug (Hyprland's `$MANAGERPID`-gated env-management skip) irrelevant.
- [ ] **Wire the merged coverage gate into CI too**, now that Tier B already
  is (`.github/workflows/ci.yml`'s `tier-b` job). `scripts/coverage.sh merged`
  passes locally (92%+); it's never been run in CI, same story Tier B was in
  until 2026-08-30.

## Intentionally not planned

- **A second Linux distro.** wsmr has no packaging story beyond the Arch
  `PKGBUILD` in `arch/`, and the only compositor it's been run against for
  real (Hyprland) is itself heavily Arch-ecosystem-centric in practice.
  Cross-distro testing isn't worth the effort until there's a packaging
  reason to care.
- **Compositors other than Hyprland.** The unit graph and env-delta logic
  are compositor-agnostic by design (same as upstream `uwsm`), but nothing
  else has ever been run through wsmr for real. Treat sway/niri/river/labwc
  as completely untested, not "should work in theory."
