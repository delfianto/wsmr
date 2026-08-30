# Arch Linux files

This directory contains two separate tools:

- [Local packaging](packaging.md) builds and installs `/usr/bin/wsmr` plus a
  Hyprland display-manager entry.
- [The real-hardware harness](e2e.md) runs a wsmr-managed Hyprland session in
  a disposable user account.

The packaging files are useful on their own. The hardware harness is for
maintainers investigating login, DRM, input, portals, and logout on an actual
machine.

Current hardware status and findings are summarized in
[Known issues](../docs/known-issues.md). Remaining manual checks are tracked in
[Open work](../docs/todo.md).
