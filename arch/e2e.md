# Real-hardware testing

This procedure runs a real Hyprland session in a disposable account. It avoids
changing the primary user's home or systemd user environment.

The scripted assertions live in `scripts/e2e-harness.sh`. Its three stages are
run before login, during the session, and after logout.

## 1. Create the test account

As root:

```sh
useradd -m -s /bin/bash wsmr
passwd wsmr
loginctl enable-linger wsmr
```

The documented account name is `wsmr`. Strings such as `wsmr-e2e` name files
and install paths, not the Linux user.

Linger allows the account's systemd user manager to run during harness stages
outside an interactive login.

## 2. Choose the binary

Use one of these methods.

### Test the installed package

Build and install the package from [packaging.md](packaging.md). The session
picker entry is `Hyprland (wsmr-managed)` and it runs `/usr/bin/wsmr`.

### Test the current checkout without packaging

From the repository root, run as root:

```sh
./arch/e2e-install.sh
```

This builds with `cargo build --release --locked`, installs a versioned binary
under `/usr/local/libexec/wsmr-e2e/`, and creates the session entry
`Hyprland (wsmr E2E)`.

Rerun the installer after code changes.

## 3. Configure Hyprland

First inspect the new account. CachyOS with Noctalia may already copy a full
Lua-based Hyprland configuration from `/etc/skel`. If it does, use that
isolated default and do not overlay the repository's minimal config.

On systems that provide no usable default for new users:

```sh
install -Dm0644 -o wsmr -g wsmr \
    arch/session/hyprland-e2e.conf /home/wsmr/.config/hypr/hyprland.conf
```

Review the `monitor` line in that file before login. It is hardware-specific.

The minimal config includes `exec-once = wsmr finalize`. If you use the
versioned E2E install and `wsmr` is not in the test user's `PATH`, replace
`wsmr` in that line with the exact path printed by `e2e-install.sh`. If you use
a distro config, add the same hook before disabling Hyprland's own environment
updates; an existing `uwsm finalize` hook is not a substitute.

Add the verified Hyprland environment workaround:

```sh
install -d -m0755 -o wsmr -g wsmr /home/wsmr/.config/wsmr
printf '%s\n' 'export HYPRLAND_NO_SD_VARS=1' \
    > /home/wsmr/.config/wsmr/env-hyprland
chown wsmr:wsmr /home/wsmr/.config/wsmr/env-hyprland
```

## 4. Run the three harness stages

From the repository root, as root:

```sh
scripts/e2e-harness.sh prepare --user wsmr
```

Log in as the test user and choose the display-manager entry matching the
binary selected above. While the wsmr-managed session is active:

```sh
scripts/e2e-harness.sh verify --user wsmr
```

After logout:

```sh
scripts/e2e-harness.sh post-logout --user wsmr
```

Each stage prints assertions and returns a useful exit status. The harness also
records the relevant component versions during preparation.

## VT warning

If switching to an unused VT starts kmscon, do not launch the compositor from
that console. Start a plain getty first:

```sh
systemctl start getty@tty2.service
```

Then switch to that VT. See [the kmscon issue](../docs/known-issues/kmscon-seat-conflict.md).

## Remaining manual scenarios

Two checks still require a person at a real console:

- kill the compositor after it reaches readiness and confirm full cleanup;
- force the display manager to terminate a login in progress and confirm
  cleanup.

Equivalent stub-compositor failures already run in Tier B. These tasks are
about real DRM, seat, and display-manager behavior.

## Remove the test setup

These commands permanently remove the disposable account and the optional E2E
install. Confirm the account name before running them:

```sh
userdel -r wsmr
rm -r /usr/local/libexec/wsmr-e2e
rm /usr/share/wayland-sessions/wsmr-e2e.desktop
```

They do not uninstall the Arch package or remove `/usr/bin/wsmr`.
