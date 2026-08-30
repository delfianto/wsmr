# One-off Hyprland cleanup crash

Hyprland itself crashed once during an ordinary logout on the test machine.
This was not the portal crash described on the adjacent page.

The coredump pointed into Hyprland's own compositor cleanup path, through
window-group and decoration teardown. wsmr had only asked systemd to stop the
compositor service; the fault occurred inside Hyprland while it was destroying
its window state.

This event has important limits:

- it happened once;
- it was not deliberately reproduced;
- it was not compared with an uwsm session; and
- it was not filed upstream.

If it occurs again, preserve the coredump and journal, reproduce it without
unrelated session changes, and report it to Hyprland. There is currently no
wsmr workaround to document.
