# uwsm core analysis (porting target for wsmr)

Deep dive into **how `uwsm` implements its two core jobs** — bootstrapping a
Wayland session and launching GUI apps as systemd units — written to drive a
**functional** Rust port (not a line-by-line translation). Compositor selection
(`uwsm select` / the whiptail menu) is **out of scope**: SDDM handles that here.

References point at the upstream Python in `uwsm/` (e.g. `main.py:4719`). They
are anchors for "what behavior to reproduce," not "what code to copy."

---

## 1. Mental model

uwsm is **not a daemon**. It is a CLI that does three kinds of work:

1. **Generates systemd user units** (or ships them static) that encode the whole
   session lifecycle as a dependency graph.
2. **Acts as the glue invoked *by* those units** (`uwsm aux …`) — environment
   preparation, the compositor exec wrapper, readiness signalling, cleanup.
3. **Translates a login-shell process into the session's lifetime anchor** — the
   `uwsm start` process replaces itself with a tiny shell signal handler that
   `systemctl --user start --wait`s the session and converts SIGTERM/HUP/INT into
   a clean stop.

The genius (and the thing to preserve) is that **systemd does the actual work**:
ordering, activation, propagation of start/stop, cgroup placement. wsmr's job is
to lay down the right units and provide the right helper subcommands.

Everything funnels through the systemd **user** manager over the **session**
D-Bus bus, plus a little **system** bus (logind) and the **D-Bus daemon** itself
(activation environment).

---

## 2. The unit graph (the heart of it)

All units live in `systemd/user/` (templated `@` units keyed by the escaped
compositor id `%I`). Reproduce this graph faithfully — the behavior emerges from
it.

```
              wayland-session-bindpid@PID.service   (Type=exec: waitpid -e PID)
                        │ OnSuccess/OnFailure
                        ▼
   uwsm start ─exec──► signal-handler.sh ─► systemctl --user start --wait
                                              wayland-session-envelope@ID.target
                                                       │ BindsTo + Before
                        ┌──────────────────────────────┴───────────────┐
                        ▼                                               ▼
        wayland-wm-env@ID.service                        wayland-wm@ID.service
        (oneshot, RemainAfterExit)                       (Type=notify, NotifyAccess=all)
        ExecStart  = aux prepare-env -- ID               ExecStart = aux exec -- ID
        ExecStopPost = aux cleanup-env                    EnvironmentFile = env_session.conf
                        │ BindsTo                                         │ BindsTo
                        ▼                                                 ▼
        wayland-session-pre@ID.target  ───────────►  wayland-session@ID.target
        (BindsTo graphical-session-pre.target)        (BindsTo graphical-session.target)
                                                          │ Wants
                                   ┌──────────────────────┼───────────────────────┐
                                   ▼                       ▼                       ▼
                wayland-session-waitenv.service   wayland-session-          (stock graphical-
                (oneshot: aux waitenv,            xdg-autostart@ID.target    session.target
                 gates graphical-session.target)  (BindsTo                   reached → apps,
                                                   xdg-desktop-              slices activate)
                                                   autostart.target)

   ANY failure / success / manual:  wayland-session-shutdown.target
     (Conflicts with graphical-session{,-pre}.target + xdg-desktop-autostart.target)
     → systemd tears the whole graph down
```

Supporting units:

- **`wayland-session-envelope@.target`** — brought up first, `BindsTo` both the
  env service and wm service, `PropagatesStopTo` the wm service, lives across the
  entire startup→runtime→shutdown span. This is the unit `uwsm start` actually
  starts (so it can wait for *teardown* to finish, not just the compositor).
- **`wayland-wm-app-daemon.service`** (`Type=exec`, `aux app-daemon`) — optional
  fast path for `uwsm app` (see §6).
- **slices** `app-graphical.slice`, `background-graphical.slice`,
  `session-graphical.slice` — `PartOf=graphical-session.target`, nested under the
  stock `app.slice`/`background.slice`/`session.slice`. Apps land here.

Key directives to carry over verbatim in spirit: `Type=notify` +
`NotifyAccess=all` on the wm service; `OnSuccess`/`OnFailure` →
`wayland-session-shutdown.target` with `JobMode=replace-irreversibly`;
`Conflicts=`/`Before=` shutdown target on everything; `CollectMode=inactive-or-failed`.

**Two rungs.** Units are written either to the **runtime** rung
(`$XDG_RUNTIME_DIR/systemd/user/`) or the **home** rung
(`$XDG_CONFIG_HOME/systemd/user/`), selected by `-U`/`$UWSM_UNIT_RUNG`. On start,
the *other* rung's managed files are removed. (`main.py:4832-4840`,
`get_unit_path` `main.py:1117`.)

---

## 3. Flow A — session bootstrap (`uwsm start <compositor>`)

Dispatch: `main.py:4719`. Sequence:

1. **(optional) gate on system `graphical.target`** via the *system* bus
   (`wait_for_unit`, `main.py:4754`). Warn or abort per `--gst-*` flags.
2. **Resolve the compositor** → `fill_comp_globals()` (`main.py:3965`). Produces
   `CompGlobals`: final `cmdline`, internal `id` (basename of arg0),
   `id_unit_string` (systemd-escaped), `desktop_names` (XDG_CURRENT_DESKTOP list),
   `name`, `description`, `bin_id` (sanitized id for shell function/plugin names).
   Source can be a bare executable, a path, or a `wayland-sessions` desktop entry
   (whose `Exec`/`DesktopNames`/`Name` are parsed; entries that themselves call
   `uwsm start` are detected and unwrapped). **For wsmr the common case is a bare
   compositor command from SDDM's session**, so the executable branch
   (`main.py:4292`) is the priority; desktop-entry resolution can come later.
3. **Refuse double start** → `is_active(verbose_active=True)` (`main.py:1189`):
   bail if a `wayland-wm@*.service` or any `graphical-session*` target is active.
4. **Generate units + drop-ins** → `generate_dropins()` (`main.py:1389`),
   `generate_tweaks()` (`main.py:1548`). The crucial one is the per-compositor
   `50_custom.conf` drop-in carrying the real compositor cmdline and metadata;
   `update_unit` (`main.py:1275`) only writes when content differs and flips
   `UnitsState.changed`.
5. **`reload_systemd()`** (`main.py:890`) if anything changed (D-Bus
   `Manager.Reload`, poll `ListJobs` until the job clears). `-o`/`--dry-run` exit
   here.
6. **Start the PID bind**: `systemctl --user start
   wayland-session-bindpid@$$.service` on uwsm's own PID (`main.py:4861`).
7. **Snapshot environments to runtime dir**:
   - `env_login` — the *entire* current env, NUL-separated (`save_env`,
     `main.py:2519`). Consumed once by `prepare-env`.
   - `env_session.conf` — only the `Varnames.session_specific` set
     (`XDG_SEAT/SEAT_PATH/SESSION_ID/SESSION_PATH/VTNR`), newline-separated, for
     units' `EnvironmentFile=` (`main.py:4876`).
8. **Become the session anchor**: dup stdout/stderr to fd 3/4, then
   `execlp("systemd-cat", … , sh, signal-handler.sh,
   "wayland-session-envelope@<id>.target")` (`main.py:4912`). uwsm the Python
   process is gone; its PID now *is* the shell signal handler.

### signal-handler.sh (`uwsm-libexec/signal-handler.sh`)

- Traps TERM/HUP/INT.
- `start()`: forks `systemctl --user start --wait $UNIT` inside a subshell that
  first `trap '' TERM HUP INT` (so the signal storm at logout can't kill it);
  records its PID.
- On signal: `stop()` forks `systemctl --user stop $UNIT` (also signal-protected),
  then `finish()` waits the start pid and exits with its RC.
- fd 3/4 carry human-readable messaging *past* `systemd-cat` (which eats fd 1/2
  into the journal). The `UWSM_SH_NO_STDOUT` env gates that.

Net effect: the login shell stays alive exactly as long as the session, and a
logout signal becomes a graceful `systemctl stop`.

### What systemd then does

`envelope@.target` pulls up `wayland-wm-env@<id>.service` (during
`graphical-session-pre.target`) and `wayland-wm@<id>.service`. The env service
runs `aux prepare-env` (§4); the wm service runs `aux exec` which preps readiness
and execs the compositor (§5). When `WAYLAND_DISPLAY` shows up,
`graphical-session.target` is reached, slices + xdg-autostart activate. Any unit
failing, or the bindpid exiting, or a manual `systemctl start
wayland-session-shutdown.target`, conflicts the session targets and brings it all
down — at which point `wayland-wm-env@.service`'s `ExecStopPost=aux cleanup-env`
runs.

---

## 4. Environment lifecycle (prepare-env / finalize / cleanup-env)

This is uwsm's subtlest machinery and the part most worth getting right. The
contract: **the delta a compositor's environment introduces is pushed into the
systemd user-manager + D-Bus activation environments, recorded, and later undone.**

### prepare-env (`aux prepare-env`, runs in `wayland-wm-env@.service`)

`prepare_env()` `main.py:2682` + `uwsm-libexec/prepare-env.sh`:

1. Load `env_login` (delete after). If absent, the shell profile will be sourced
   instead (the `__LOAD_PROFILE__` flag).
2. **Deduce session identity if missing**: read foreground VT from
   `/sys/class/tty/tty0/active` (`get_fg_vt` `main.py:2567`), then query logind
   over the *system* bus to map VT→`(XDG_SESSION_ID, XDG_SEAT)`
   (`get_session_by_vt` `main.py:2592`, `ListSessionsEx` + session `VTNr`/`Leader`
   properties). If no bindpid is active, start one on the session leader PID as a
   best-effort lifetime bind. Re-save `env_session.conf`.
3. Read the **current** systemd activation env (`get_systemd_vars`), save it as
   `env_pre` (for restoration on cleanup). Merge `env_pre | env_login`.
4. Write an aux-vars file exporting `__SELF_NAME__`, `__WM_ID__`,
   `__WM_BIN_ID__`, `__WM_DESKTOP_NAMES__`, `__LOAD_PROFILE__`, a random mark, and
   `IN_UWSM_ENV_PRELOADER=true`.
5. Run `prepare-env.sh` under `/bin/sh` with the merged env, passing the aux-vars
   file + any compositor shell plugins (`uwsm/plugins/<bin_id>.sh`). The script:
   - sources `/etc/profile` + `~/.profile` (only if login env present),
   - sets XDG base-dir defaults + `XDG_CURRENT_DESKTOP`/`XDG_SESSION_DESKTOP`/
     `XDG_MENU_PREFIX`/`XDG_SESSION_TYPE=wayland`,
   - runs optional `quirks_<bin_id>` plugin hook,
   - sources `uwsm/env` and `uwsm/env-<desktop>` (+ `.d/`) files across the whole
     XDG config/data hierarchy in **reverse** (increasing-priority) order,
   - prints the random mark then `exec env -0` to dump the resulting env
     NUL-separated.
6. Back in Python: split on the mark, parse `env_post`, compute
   `set_env = env_post − env_pre`, then **+`always_export`, −`never_export`,
   −`always_unset`** (the `Varnames` sets, `main.py:141`). Compute `unset` =
   (`env_pre` keys − `env_post` keys + `always_unset`) ∩ current systemd vars.
   Append `set_env` keys (−`never_cleanup`) to the cleanup list. Push via
   `set_systemd_vars` / `unset_systemd_vars`.

### finalize / autoready (readiness signalling)

`wayland-wm@.service` is `Type=notify`. Readiness can be declared two ways:

- **Autoready (automatic):** the `aux exec` handler (`main.py:5066`) forks a
  watcher (double-fork to reparent away from the compositor), which waits for
  `WAYLAND_DISPLAY` + `$UWSM_WAIT_VARNAMES` to appear in the systemd activation
  env (`waitenv` `main.py:4464`), syncs the delta to D-Bus, appends cleanup, then
  `execlp("systemd-notify", "READY=1", "NOTIFYACCESS=exec")`. The parent execs the
  compositor. So **a compositor that merely puts `WAYLAND_DISPLAY` into the
  systemd/D-Bus activation env gets readiness for free.**
- **Explicit:** the compositor runs `uwsm finalize [VARS…]` (`main.py:2424`),
  equivalent to `dbus-update-activation-environment` + `systemctl import-environment`
  + `systemd-notify READY=1 NOTIFYACCESS=exec`. Extra vars come from
  `$UWSM_FINALIZE_VARNAMES`.

Independently, `wayland-session-waitenv.service` (`aux waitenv`) gates
`graphical-session.target`: it waits for the same vars and either succeeds or
times out → shutdown.

`NotifyAccess` is narrowed from `all` to `exec` once ready, so stray children
can't spoof readiness.

### cleanup-env (`aux cleanup-env`, `ExecStopPost` of the env service)

`cleanup_env()` `main.py:2922`: read cleanup list(s), **∪ `always_cleanup`,
− `never_cleanup`, ∩ current systemd vars, − `env_pre` keys**; unset those; then
restore `env_pre`; remove the runtime files. Refuses to run if a compositor is
still active (`main.py:5056`).

### dbus-broker nuance (don't miss this)

`set_systemd_vars` (`main.py:917`) always calls systemd `Manager.SetEnvironment`,
and **additionally** mirrors to the D-Bus daemon's
`UpdateActivationEnvironment` *unless* `dbus.service` is `dbus-broker.service`
(broker shares systemd's activation env). The port must check the running D-Bus
implementation and skip the redundant call for dbus-broker. Same for unset.

---

## 5. The compositor exec wrapper (`aux exec`)

`main.py:5066`. Runs as `ExecStart` of `wayland-wm@.service`:

1. Snapshot systemd vars (`env_pre` for delta).
2. Fork the autoready watcher (double-fork) described in §4.
3. Parent `execlp`s `CompGlobals.cmdline` — i.e. **the compositor replaces the
   wrapper process**, inheriting the unit's cgroup, `EnvironmentFile=env_session.conf`,
   and `$NOTIFY_SOCKET`.

The fork/exec discipline matters: the watcher must not be in the compositor's
process group/cgroup lifecycle, hence reparenting via double-fork.

---

## 6. Flow B — launching GUI apps (`uwsm app …`)

Dispatch: `main.py:4972` → `app()` `main.py:3335`. Default unit type **scope**
(`UWSM_APP_UNIT_TYPE` overrides), default slice **`a`** = `app-graphical.slice`.

Steps:

1. **Classify arg0** via `MainArg` (`main.py:188`): desktop entry
   (`foo.desktop` / `foo.desktop:action` / a path to one), or a bare executable
   (with/without path).
2. **Desktop entry path** (`main.py:3372`):
   - Resolve the entry (by id across `applications` dirs, or directly from a
     path), run basic validity checks (`check_entry_basic`/`check_entry_showin`,
     `main.py:424`/`:501` — `TryExec`, `Hidden`, `OnlyShowIn`/`NotShowIn` vs
     `XDG_CURRENT_DESKTOP`).
   - Pull metadata: `SourcePath=` unit property, `Terminal=` flag, `Path=`
     workdir, `app_name` from entry id, description from `Name`/`GenericName`.
   - **Expand `Exec`** via `gen_entry_args()` (`main.py:2999`): handle field
     codes `%f %F %u %U %c %k %i`, drop deprecated `%d %D %n %N %v %m`, unescape
     `%%`. With multiple file/url args and a single-valued `%f`/`%u`, it produces
     **a list of arg-lists → one unit instance per file** (recursive forked
     `app()` calls + a poll loop, `main.py:3450`).
3. **Bare executable path** (`main.py:3504`): adopt `DESKTOP_ENTRY_*` env vars for
   naming/`SourcePath` if present; verify the command exists.
4. **Terminal apps** (`main.py:3537`): resolve a terminal entry
   (`find_terminal_entry` `main.py:3170`) and assemble
   `terminal_cmdline + exec-arg + app cmdline`, honoring `TerminalArg*` keys
   (`--app-id`/`--title`/`--dir`/`--hold`).
5. **Unit naming** (`main.py:3659`): explicit `--unit-name` (validated, ≤255), or
   auto `app-<desktop>-<cmd>-<hex8>.scope` / `app-<desktop>-<cmd>@<hex8>.service`,
   with systemd escaping (`simple_systemd_escape` `main.py:1015`) and careful
   255-byte truncation.
6. **Build `systemd-run`** (`main.py:3729`):
   ```
   systemd-run --user
     {--scope | --property=Type=exec --property=ExitType=cgroup
                --setenv=<session_specific vars>}
     [--property=StandardOutput/Error=null …]      # --silent out|err|both
     [--property=<user props>]
     --slice=<slice> --unit=<name> --description=<desc>
     --quiet --collect {--working-directory=<wd> | --same-dir}
     -- <terminal cmdline> <app cmdline>
   ```
   - **scope**: app runs in the *caller's* lifecycle, registered into the slice.
   - **service**: a managed unit (`Type=exec`, `ExitType=cgroup`); session
     identity vars injected via `--setenv` since a fresh service won't inherit
     them.
   - Then `os.execlp` the `systemd-run` (or `Popen` when forking instances, or
     return the argv when invoked by the app-daemon).
   - Before exec, `DESKTOP_ENTRY_*` vars are stripped from the environment so they
     don't leak into the launched app.

### app-daemon (latency optimization)

`app_daemon()` `main.py:3815` (unit `wayland-wm-app-daemon.service`): a
long-lived process that reads NUL-separated argv from the `uwsm-app-daemon-in`
FIFO, runs the same arg parsing + `app(return_cmdline=True)`, and writes **shell
code** to `uwsm-app-daemon-out` (`exec systemd-run …`, or `… & … & wait` for
multi-instance). A thin `uwsm-app` shell client uses it to avoid paying Python
startup per launch. Supports `ping`→`pong`, `stop`. **Port this last** — it's an
optimization, not core behavior; `uwsm app` works without it.

---

## 7. D-Bus surface actually used (`uwsm/dbus.py`)

Small and well-defined — a good seam for a typed Rust client (`zbus`):

| Bus | Service / interface | Calls used |
|-----|--------------------|-----------|
| session | `org.freedesktop.systemd1.Manager` | `Reload`, `ListJobs`, `GetUnit`, `ListUnitsByPatterns`, `StopUnit`, `SetEnvironment`, `UnsetEnvironment`, `Get(Environment)` |
| session | per-unit `org.freedesktop.DBus.Properties` | unit props (`TimeoutStartUSec`, `NotifyAccess`, `Id`) |
| session | `org.freedesktop.DBus` (daemon) | `UpdateActivationEnvironment` (skip for dbus-broker) |
| system | `org.freedesktop.login1.Manager` | `ListSessions(Ex)`, `GetSession`, session props (`VTNr`, `Leader`) |
| session | `org.freedesktop.Notifications` | `Notify` (user-facing errors) |

Plus non-D-Bus syscalls/tools: `systemctl --user start/stop` (subprocess),
`systemd-run` (exec), `systemd-notify` (exec), `systemd-cat` (exec),
`waitpid(1)` or `aux waitpid` fallback, `/sys/class/tty/tty0/active`,
`fork`/`execlp`/`dup2`.

---

## 8. Proposed Rust architecture for wsmr

Map by **function**, isolate every side effect behind a trait so logic is
unit-testable on macOS (where none of this runs).

```
src/
  main.rs              # clap dispatch → subcommand modules
  cli.rs               # clap derive: start | stop | finalize | app | aux {…} | check {…}
  comp.rs              # CompGlobals: resolve compositor → id, cmdline, desktop names (fill_comp_globals)
  units/
    mod.rs             # unit graph definitions (the §2 templates as const/templated strings)
    generate.rs        # write/diff drop-ins + rung management (update_unit/generate_dropins)
    escape.rs          # systemd unit-string escaping (simple_systemd_escape/char2cesc)
  session/
    start.rs           # Flow A orchestration + env snapshot + exec signal-handler
    stop.rs            # stop_wm / shutdown target
    signal_handler.rs  # the trap/fork/wait anchor (may stay a shipped sh script initially)
  env/
    prepare.rs         # prepare_env: VT/logind deduction, shell-fragment sourcing, delta
    finalize.rs        # finalize + autoready watcher
    cleanup.rs         # cleanup_env
    varnames.rs        # the Varnames sets (always_export/never_export/always_unset/…)
    shell.rs           # run prepare-env.sh, parse `env -0` dump
  app/
    launch.rs          # app(): MainArg, slices, scope/service, systemd-run assembly
    desktop_entry.rs   # entry lookup + validity + Exec field expansion (gen_entry_args)
    daemon.rs          # app-daemon FIFO server (last)
  sysd/
    dbus.rs            # typed zbus client mirroring dbus.py (the §7 table)
    proc.rs            # systemctl/systemd-run/systemd-notify/systemd-cat wrappers
    logind.rs          # VT→session/seat
  util/
    xdg.rs             # XDG base dirs + desktop-entry discovery
    pidfd.rs           # waitpid-equivalent (pidfd_open/waitid)
```

### Crate candidates (decide in `m11-ecosystem` before adding)

- **CLI:** `clap` (derive) — mirror the subcommand tree exactly.
- **D-Bus:** `zbus` (pure-Rust, async or blocking). Generate typed proxies for
  systemd Manager, logind Manager, the D-Bus daemon, Notifications.
- **XDG / desktop entries:** `xdg` or `etcetera` for base dirs;
  `freedesktop-desktop-entry` for `.desktop` parsing (verify it exposes
  `Exec`/`TryExec`/`OnlyShowIn`/`DesktopNames`/actions — may need a thin layer).
- **systemd notify:** `sd-notify` crate (small) or hand-rolled `$NOTIFY_SOCKET`
  writer to avoid `os.execlp("systemd-notify")`.
- **Process/pidfd:** `nix` for `fork`/`exec`/`dup2`/`pidfd`/`waitid`/signals.
- **Errors:** `thiserror` for typed library errors; `anyhow` at the binary edge.
- **Async:** only if `zbus` async is chosen; the watcher + job-polling are
  naturally async. A blocking design is also viable and simpler — decide early.

### Things that are genuinely harder in Rust than Python

- **fork + double-fork + execlp** (the autoready watcher and `aux exec`): doable
  with `nix`, but mind async-signal-safety between fork and exec, and don't hold
  locks/allocator state across `fork`. Consider replacing the double-fork-reparent
  watcher with a separate short-lived unit or a `pidfd`-based helper.
- **Process self-replacement** (`uwsm start` → signal-handler.sh): simplest first
  cut is to **keep shipping `signal-handler.sh` and `prepare-env.sh` as-is** and
  `execvp` into them. Port them to Rust later (or never — they're fine as POSIX
  sh). Don't block the port on rewriting shell glue.
- **D-Bus typing**: systemd's variant-heavy return types (`ListUnitsByPatterns`,
  `ListSessionsEx`) need careful zbus struct modeling.
- **Environment delta semantics**: the set algebra in §4 is the spec — port it
  with property tests; it's pure logic and fully testable on macOS.

---

## 9. Suggested milestones (core-first, selector excluded)

1. **M0 — skeleton**: `clap` tree (`start`/`stop`/`app`/`aux`/`finalize`/`check`),
   `CompGlobals` resolution for the **bare-executable** case, `Varnames` sets,
   systemd-escape — all pure, all unit-tested on macOS.
2. **M1 — D-Bus client**: typed `zbus` wrappers for the §7 table; integration-test
   later in a Linux container (deferred).
3. **M2 — unit generation**: emit the §2 graph + `50_custom.conf`, rung handling,
   diff-on-write, reload. Verify generated unit text against the upstream
   templates byte-for-byte where sensible.
4. **M3 — bootstrap happy path**: `start` → bindpid → snapshot env → exec the
   (initially shell) signal handler; `aux prepare-env` (reuse `prepare-env.sh`) →
   delta → push to systemd/D-Bus; `aux exec` + autoready; `aux waitenv`. Bring up
   a real compositor in a Linux VM.
5. **M4 — stop/cleanup**: `stop_wm`, shutdown target, `cleanup-env` restore.
6. **M5 — `uwsm app`**: bare exec + scope/service + slice + `systemd-run`; then
   desktop-entry resolution + `Exec` field expansion + multi-instance + terminal.
7. **M6 — polish**: `finalize` explicit path, dbus-broker detection,
   `check may-start`, notifications.
8. **M7 (optional) — app-daemon** FIFO fast path.

The selector, plugins/quirks, tweaks drop-ins, and `fumon`/`ttyautolock` extras
are explicitly **non-goals** for the core port.
