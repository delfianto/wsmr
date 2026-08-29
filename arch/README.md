# arch/ — Arch/CachyOS packaging and disposable-user E2E harness scaffolding

> **Status (2026-08-29):** `PKGBUILD` built and installed on the real
> machine; the disposable `wsmr` user exists with linger enabled; a real
> wsmr-managed Hyprland session has been started, verified against most of
> `docs/fix-plan.md`'s Phase 7 checklist, and stopped cleanly. See Phase 7's
> evidence section there for the full results, including two real findings
> from this pass: a `kmscon`↔Hyprland seat-ownership conflict (with a
> working VT-based workaround) and an environment-restoration gap on
> `wsmr stop`. Still not done: any login actually mediated by a display
> manager (this system's greeter is `greetd` + `noctalia-greeter`, not
> SDDM), and the P7-02 harness script (everything so far was run by hand).

This directory supports two related but separate things:

1. **Packaging** (`PKGBUILD`) — a normal Arch package for wsmr, installing
   `/usr/bin/wsmr`. Useful on its own; not what the disposable E2E test
   setup below needs.
2. **Disposable-user E2E setup** (`e2e-install.sh`, `session/`) — the
   real-Hyprland-session infrastructure `docs/fix-plan.md`'s Phase 7
   requires before its "real machine" gate can be considered met. This is
   *setup*, not verification: it gets you to the point where you can log in
   as the disposable test user. The actual verification harness (a
   prepare/verify/post-logout script, the in-session assertions, the
   failure-injection scenarios) is not written yet — see "What's not here"
   below.

Read `docs/fix-plan.md`'s Phase 7 section alongside this file; this
directory exists to satisfy its prerequisites, and nothing here should
drift from that checklist without updating both.

## Packaging (PKGBUILD)

Standard local build, from a checkout of this repo:

```sh
cd arch
makepkg -si
```

Builds via `cargo build --frozen --release` against the checked-in
`Cargo.lock`, runs `cargo test` in `check()` (skip with `makepkg --nocheck`
for a faster iterative build), and installs `/usr/bin/wsmr` plus docs
(`docs/fix-plan.md`, `docs/architecture.md`, `docs/known-issues.md`)
under `/usr/share/doc/wsmr/`, plus a Hyprland wayland-sessions entry
(`/usr/share/wayland-sessions/hyprland-wsmr.desktop`, from
`arch/session/hyprland-wsmr.desktop`) so the package is actually usable to
log in with, not just a binary. `pkgver()` reads `Cargo.toml` directly so it
can't drift from the crate version.

The session entry is Hyprland-specific — wsmr itself is compositor-agnostic
like uwsm, but this package targets a Hyprland system, the same way the real
`hyprland-uwsm.desktop` is shipped by the `hyprland` package rather than by
`uwsm` itself. A different compositor needs its own entry with the same
`Exec=wsmr start -e -D Hyprland hyprland.desktop`-style shape, just with a
different compositor id.

`.SRCINFO` is checked in for AUR-style tooling; regenerate it after editing
`PKGBUILD` with `makepkg --printsrcinfo > .SRCINFO`.

**Do not use this for the E2E setup below.** Installing this package puts
wsmr at `/usr/bin/wsmr` — the same path a real system install would use —
which is exactly the ambiguity the disposable test path is designed to
avoid. Use `e2e-install.sh` instead; it never touches `/usr/bin`.

## Disposable-user E2E setup

Everything below sets up the **disposable test identity**
`docs/fix-plan.md`'s Phase 7 prerequisites require: *"Do not use the
primary user's active uwsm-managed desktop for the first run."* Every step
is scoped to a brand-new user account plus paths under
`/usr/local/libexec/wsmr-e2e/` and `/usr/share/wayland-sessions/` — nothing
here reads or writes the primary user's home directory, `/usr/bin/wsmr`, or
the primary user's actual uwsm-managed session.

### 1. Create the disposable user

As root:

```sh
useradd -m -s /bin/bash wsmr
passwd wsmr
loginctl enable-linger wsmr
```

(Whether your display manager can log this account in without an
interactive password prompt is a DM-specific setting — set a real password
unless you've already decided how you want to handle that.)

`enable-linger` is what lets `systemd --user` for this account start at
login (and, for later non-interactive harness stages, without one).

**On this machine the account is named `wsmr`, not `wsmr-e2e`.** The
`wsmr-e2e` string elsewhere in this document only ever names *paths and
files* (`/usr/local/libexec/wsmr-e2e/`, `wsmr-e2e.desktop`) — none of them
encode a username, so nothing else below needed to change. Every command
from here on uses the real account name, `wsmr`.

### 2. Get a binary + session entry in place

Two ways to do this — pick one:

- **Already built and installed `arch/PKGBUILD`?** Then `/usr/bin/wsmr` and
  `/usr/share/wayland-sessions/hyprland-wsmr.desktop` already exist (verify:
  `pacman -Qi wsmr`, `pacman -Ql wsmr`). Nothing further to do here — skip
  straight to step 3. The disposable account's isolation comes from being a
  *separate Linux user* with its own `systemd --user`/home/environment, not
  from which binary path it happens to run, so testing against the real
  `/usr/bin/wsmr` is exactly as safe here as a dedicated test path would be
  — and it's what the account will actually be running day to day. This is
  the path actually used and verified on 2026-08-29 (see `docs/fix-plan.md`
  Phase 7).
- **Haven't built the package, or want a path that never touches
  `/usr/bin/wsmr`** (e.g. to test a work-in-progress build without a full
  `makepkg -si` cycle): run `./arch/e2e-install.sh` as root instead. It does
  a lighter `cargo build --release --locked` from the crate root (no
  `cargo test`, no pacman DB registration) and installs to
  `/usr/local/libexec/wsmr-e2e/<version-from-Cargo.toml>/wsmr`, then
  generates `/usr/share/wayland-sessions/wsmr-e2e.desktop` from
  `session/wsmr-e2e.desktop.in` pointing at that exact path. Rerun it after
  any code change — safe and idempotent. The generated entry is modeled on
  the real, package-shipped `hyprland-uwsm.desktop`
  (`/usr/share/wayland-sessions/`, from the `uwsm` Arch package): same
  `<tool> start -e -D Hyprland hyprland.desktop` shape, just pointed at the
  E2E binary and named `Hyprland (wsmr E2E)` so it's unmistakable in a
  display manager's session list. Not yet exercised end to end.

Either way, when you get to step 5 you'll pick whichever session name
("Hyprland (wsmr-managed)" or "Hyprland (wsmr E2E)") matches the path you
used — and record which one you used in `docs/fix-plan.md`'s Phase 7
evidence.

### 3. Give the test user their own, isolated Hyprland config

**Read this before running it — the assumption below turned out to be
wrong on the actual target system.** This account's `/etc/skel`-provided
home already ships a full, working Hyprland config (CachyOS's
`cachyos-hypr-noctalia` bundle uses Hyprland's native **Lua** config
support — `~/.config/hypr/hyprland.lua` + `config/*.lua` — not a plain
`hyprland.conf`). Installing `hyprland-e2e.conf` on top of that, as
originally suggested here, doesn't shadow it as expected and just adds
confusion. **On a CachyOS/Noctalia system, skip this step entirely** and
let the account use its own already-isolated `/etc/skel` default (it
already isn't the primary user's personal config, and doesn't invoke
`/usr/bin/uwsm` to start). Only run the command below on a system that
*doesn't* ship its own default Hyprland config for new accounts:

```sh
install -Dm0644 -o wsmr -g wsmr \
    arch/session/hyprland-e2e.conf /home/wsmr/.config/hypr/hyprland.conf
```

`session/hyprland-e2e.conf` is a deliberately minimal config — no imports
from the primary user's dotfiles, no reliance on `/usr/bin/uwsm`. Read the
comments at its top: the `monitor` line is hardware-dependent and will need
adjusting for your real output(s) before the first login.

### 4. Record versions

Before the first real run, capture what you're testing against — this goes
into `docs/fix-plan.md`'s Phase 7 evidence section, not just this README:

```sh
pacman -Q systemd dbus hyprland wsmr 2>/dev/null
uname -r
wsmr --version   # or /usr/local/libexec/wsmr-e2e/*/wsmr --version, if that's the path in use
```

### 5. Log in

Select **"Hyprland (wsmr-managed)"** (or **"Hyprland (wsmr E2E)"**,
matching whichever path you used in step 2) in your display manager's
session picker for the `wsmr` user. **If VT-switching to an unused console
first spawns a styled console (`kmscon`) instead of a bare login prompt**,
be aware this can conflict with Hyprland for seat/input ownership — see
`docs/fix-plan.md`'s Phase 7 evidence for the symptom (mouse/keyboard
completely unresponsive) and the working fix (`systemctl start
getty@ttyN.service` on an unused VT first, then switch to *that* one, so
logind never spawns `kmsconvt@` there). From here, `docs/fix-plan.md`'s
Phase 7 checklist is what actually verifies the session — see "What's not
here."

## What's not here

This directory gets you to a login prompt for the disposable user; it does
not implement:

- The **three-stage harness script** (`prepare`/`verify`/`post-logout`)
  `docs/fix-plan.md`'s Phase 7 calls for.
- The **in-session assertions** (socket checks, unit introspection, D-Bus
  activation environment via a custom fixture, app launches, autostart,
  environment restoration, no-stale-state) as a *repeatable* script — most
  of these were checked by hand on 2026-08-29; see `docs/fix-plan.md`.
- The **failure/recovery scenarios** (compositor crash before/after
  readiness, forced termination, recovery procedure).

Those are real scripting work that, like the Tier-B smoke test in
`tests/integration/`, only earns trust by being iterated against a real
session — not something to draft blind and mark done. Write them against
this setup when you're ready, and record results in `docs/fix-plan.md`'s
Phase 7 evidence section as you go, the same way every other phase in this
repo has.

## Uninstalling the E2E setup

```sh
userdel -r wsmr
rm -rf /usr/local/libexec/wsmr-e2e
rm -f /usr/share/wayland-sessions/wsmr-e2e.desktop
```

None of this touches `/usr/bin/wsmr`, any package installed via
`arch/PKGBUILD`, or the primary user's account.
