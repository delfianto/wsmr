# arch/ — Arch/CachyOS packaging and disposable-user E2E harness scaffolding

> **Status (2026-08-30):** `PKGBUILD` built and installed on the real
> machine; the disposable `wsmr` user exists with linger enabled;
> `scripts/e2e-harness.sh`'s three stages (`prepare`/`verify`/`post-logout`)
> are built and have been run for real against a live wsmr-managed Hyprland
> session on this account, repeatedly. Two real findings from that work are
> documented in [`../docs/known-issues.md`](../docs/known-issues.md): a
> `kmscon`↔Hyprland seat-ownership conflict (working VT-based workaround)
> and a Hyprland environment-restoration gap (mitigated —
> `HYPRLAND_NO_SD_VARS=1`, verified fully fixing it). A real
> display-manager-mediated login has *also* since been verified — not on
> this disposable account, but on the primary account's own daily-driver
> desktop (`greetd` + `noctalia-greeter`, also documented in
> `known-issues.md`); this account's own login is still whatever raw-VT or
> manual method you use below. See [`../TODO.md`](../TODO.md) for exactly
> what real-hardware verification is still open — two failure scenarios
> (compositor crash after readiness, login cancellation) specifically need
> a genuine interactive console login on *this* account to test.

This directory supports two related but separate things:

1. **Packaging** (`PKGBUILD`) — a normal Arch package for wsmr, installing
   `/usr/bin/wsmr`. Useful on its own; not what the disposable E2E test
   setup below needs.
2. **Disposable-user E2E setup** (`e2e-install.sh`, `session/`) — the
   real-Hyprland-session infrastructure needed to verify wsmr against a
   real compositor on real hardware without touching your own daily-driver
   account. This is *setup* (getting you to a login prompt for the
   disposable user) plus the *harness* (`scripts/e2e-harness.sh`, run from
   outside the disposable account) that turns "log in and poke around" into
   real, repeatable, scripted assertions.

Run `scripts/e2e-harness.sh` (from the repo root, as root) for the actual
verification once you're set up here: `prepare` before logging in,
`verify` while a wsmr-managed session is live, `post-logout` after it ends.
Its own `--help`/usage output and inline comments are the checklist —
nothing external to keep in sync with.

## Packaging (PKGBUILD)

Standard local build, from a checkout of this repo:

```sh
cd arch
makepkg -si
```

Builds via `cargo build --frozen --release` against the checked-in
`Cargo.lock`, runs `cargo test` in `check()` (skip with `makepkg --nocheck`
for a faster iterative build), and installs `/usr/bin/wsmr` plus docs
(`README.md`, `docs/architecture.md`, `docs/known-issues.md`, `TODO.md`)
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

Everything below sets up a **disposable test identity** so real-hardware
verification never has to touch your own daily-driver account, at least not
for the first pass. Every step is scoped to a brand-new user account plus
paths under
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
  the path actually used and verified on 2026-08-29.
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
used.

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

Before the first real run, capture what you're testing against:

```sh
pacman -Q systemd dbus hyprland wsmr 2>/dev/null
uname -r
wsmr --version   # or /usr/local/libexec/wsmr-e2e/*/wsmr --version, if that's the path in use
```

(`scripts/e2e-harness.sh prepare` does this automatically as part of its
own baseline snapshot — see below.)

### 5. Log in, then run the harness

Select **"Hyprland (wsmr-managed)"** (or **"Hyprland (wsmr E2E)"**,
matching whichever path you used in step 2) in your display manager's
session picker for the `wsmr` user. **If VT-switching to an unused console
first spawns a styled console (`kmscon`) instead of a bare login prompt**,
be aware this can conflict with Hyprland for seat/input ownership — see
[`../docs/known-issues.md`](../docs/known-issues.md) for the symptom
(mouse/keyboard completely unresponsive) and the working fix (`systemctl
start getty@ttyN.service` on an unused VT first, then switch to *that*
one, so logind never spawns `kmsconvt@` there).

From outside the disposable account (as root, from the repo root):

```sh
scripts/e2e-harness.sh prepare --user wsmr      # before logging in
# ... log in as wsmr, start a wsmr-managed session ...
scripts/e2e-harness.sh verify --user wsmr       # while the session is live
# ... log out / wsmr stop ...
scripts/e2e-harness.sh post-logout --user wsmr  # after it ends
```

Each stage prints real pass/fail assertions with a real exit code — this
*is* the checklist, not a pointer to one elsewhere.

## What's still not here

This directory plus `scripts/e2e-harness.sh` covers setup, login, and the
full prepare/verify/post-logout lifecycle. What's still not automated for
*this* disposable-account, real-hardware path (see [`../TODO.md`](../TODO.md)
for the complete list, not just the two below):

- **Compositor crash after readiness** and **login cancellation/forced
  termination** as real-hardware scenarios — both need a genuine
  interactive console login on this account, which isn't something to
  script blind from an unattended context. (The container-based Tier-B
  suite already covers the equivalent scenarios against a stub compositor —
  see `tests/integration/` — and a third real-hardware scenario, compositor
  configuration error before readiness, has been verified live and doesn't
  need this.)
- **A written, standalone recovery-procedure doc.** The evidence that
  recovery just works (no manual intervention needed) is solid; nobody's
  written it up as an operator-facing page yet.

## Uninstalling the E2E setup

```sh
userdel -r wsmr
rm -rf /usr/local/libexec/wsmr-e2e
rm -f /usr/share/wayland-sessions/wsmr-e2e.desktop
```

None of this touches `/usr/bin/wsmr`, any package installed via
`arch/PKGBUILD`, or the primary user's account.
