# Hotkey engine

One choice, three platforms, no double runs. This file is the whole
design; the code follows it exactly.

## Vocabulary

Choice names live in `core::hotkey` and nowhere else: `super_shift_r`,
`ctrl_shift_r`, `shift_d`. Validation rejects anything else with the
list attached. Mapping a name to OS keys stays in the platform
adapters (`hotkey_from_name` on Windows, bind lines on Linux).

## Storage

The SQLite kv store holds the `hotkey` choice. `hotkey-set` writes
it, the daemon reads it when no flag is passed, and the GUI mirrors
every save and onboarding finish into the same key. Explicit always
wins: CLI flag, then stored choice, then the default.

## Platform paths

Linux on Wayland cannot register global hotkeys at all: no protocol
exists and the Tauri global-shortcut plugin stays silent there, still
broken upstream. So the compositor owns the key: a Hyprland bind
writes `toggle` to the daemon socket. The choice names only change
which bind line onboarding and `hyprland-bind` print.

Windows registers the stored choice with `RegisterHotKey` in both
the CLI daemon and the GUI listener, rebuilt every press so remaps
apply without restarts.

## Idempotency

At most one dictation runs at a time. The GUI holds an atomic
in-flight slot: `start_dictation`, the tray entry, the hotkey loop,
and the onboarding test all claim it, and a second trigger reports
busy instead of stacking runs. The CLI daemon is sequential by
construction. Injection keeps its ticket gate underneath, so even a
double trigger could never paste twice.
