# Local Arch package

The `PKGBUILD` packages the checksum-verified x86_64 binary archive from the
matching [GitHub release](https://github.com/delfianto/wsmr/releases).

## Build and install

From the repository root:

```sh
cd arch
makepkg -si
```

The package:

- installs `/usr/local/bin/wsmr`;
- installs the documentation tree; and
- installs `hyprland-wsmr.desktop` under the Wayland sessions directory.

You can also download the ready-to-install `.pkg.tar.zst` from the release
and install it with `sudo pacman -U`. For a new release, update `pkgver` and
the archive checksum in `PKGBUILD`, then regenerate `.SRCINFO`:

```sh
makepkg --printsrcinfo > .SRCINFO
```

## Hyprland session entry

The package's session entry runs:

```text
/usr/local/bin/wsmr start -e -D Hyprland hyprland.desktop
```

wsmr itself is compositor-independent; the package entry is not. Another
compositor needs its own desktop file with the same shape and the appropriate
desktop name and command.

## Relationship to the hardware harness

The disposable user may use the packaged `/usr/local/bin/wsmr`. Linux user accounts
have separate homes, activation environments, and systemd user managers, so
that remains isolated from the primary account.

For testing an uninstalled work-in-progress binary, the harness also provides
`e2e-install.sh`, which writes a versioned binary below
`/usr/local/libexec/wsmr-e2e/`. See [Hardware testing](e2e.md).
