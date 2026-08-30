# Local Arch package

The `PKGBUILD` builds from the current checkout. It does not download a release
archive because the project has no tagged release yet.

## Build and install

From the repository root:

```sh
cd arch
makepkg -si
```

The package:

- builds with `cargo build --frozen --release`;
- runs `cargo test --frozen --release` in `check()`;
- installs `/usr/bin/wsmr`;
- installs the documentation tree; and
- installs `hyprland-wsmr.desktop` under the Wayland sessions directory.

Use `makepkg --nocheck` only when deliberately skipping tests during local
iteration.

`pkgver()` reads the version from `Cargo.toml`. The checked-in `.SRCINFO` must
still be regenerated after any `PKGBUILD` change:

```sh
makepkg --printsrcinfo > .SRCINFO
```

## Hyprland session entry

The package's session entry runs:

```text
wsmr start -e -D Hyprland hyprland.desktop
```

wsmr itself is compositor-independent; the package entry is not. Another
compositor needs its own desktop file with the same shape and the appropriate
desktop name and command.

## Relationship to the hardware harness

The disposable user may use the packaged `/usr/bin/wsmr`. Linux user accounts
have separate homes, activation environments, and systemd user managers, so
that remains isolated from the primary account.

For testing an uninstalled work-in-progress binary, the harness also provides
`e2e-install.sh`, which writes a versioned binary below
`/usr/local/libexec/wsmr-e2e/`. See [Hardware testing](e2e.md).
