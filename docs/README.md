# Documentation index

Start with the [top-level README](../README.md) for what wsmr is and why it
exists. Everything below goes deeper on one specific aspect.

| Document | What it's for |
|---|---|
| [`architecture.md`](architecture.md) | **How wsmr actually works**: design philosophy, module layout, the systemd unit graph, the start/stop lifecycle, the environment-delta machinery — with diagrams. Start here to understand the code. |
| [`known-issues.md`](known-issues.md) | **Real-world quirks found on actual hardware**: the kmscon/Hyprland seat conflict, the `xdg-desktop-portal-hyprland` SIGSEGV, Hyprland's own environment-restoration bug — plus the compositor support matrix (Hyprland tested; everything else untested). Read this before running wsmr on a real machine. |
| [`fix-plan.md`](fix-plan.md) | The live tracker: exactly what's verified vs. still open, phase by phase, with raw evidence. The source `known-issues.md` was distilled from. |

Diagrams referenced from the docs above live under [`diagrams/`](diagrams/)
as standalone SVGs.
