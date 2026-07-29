# Cross-desktop testing checklist (M6)

Fodder targets GNOME, KDE Plasma, XFCE, and Sway (X11 and Wayland). This is the
manual pass to run on each environment you can reach. Install first so the icon
and `.desktop` are in place:

```bash
./install.sh          # or ./install.sh --debug to skip the release build
```

Then confirm `~/.local/bin` is on your `PATH`.

## Per-desktop matrix

| Check | GNOME | KDE | XFCE | Sway |
|-------|:----:|:---:|:----:|:----:|
| App appears in the launcher/menu with the RSS icon | | | | |
| Launching from the menu opens the viewer | | | | |
| Window shows the Fodder icon (switcher / taskbar) | | | | |
| Tray icon appears (or degrades gracefully — see below) | | | | |
| Tray left-click opens the viewer | | | | |
| Tray menu: Open / Refresh all / Quit work | | | | |
| Desktop notification on new articles shows the app icon | | | | |
| Clicking a notification opens the article | | | | |
| Daily reminder fires (set it a couple minutes out, viewer closed) | | | | |
| WebKit reader loads a live page; back/forward work | | | | |

## Tray specifics

- **KDE / XFCE / Sway (with a StatusNotifier host, e.g. waybar)**: the tray icon
  should appear. If it doesn't, check the daemon log for
  `system tray registered`.
- **Vanilla GNOME Shell** has no SNI tray. Expected graceful degrade: the daemon
  logs `no system tray available (...); continuing without it` and keeps
  polling/notifying. Verify you can still open the viewer via a **notification
  click** or by running **`fodder`** again. (An AppIndicator extension will make
  the tray show; without it, degrade is the correct behavior.)

## Single-instance / lifecycle

- Start `fodderd`, then run it again → the second invocation logs
  `daemon already running; requesting it open the viewer` and exits.
- With the viewer open, run `fodder` again → the duplicate is rejected and the
  existing window is raised (daemon logs `duplicate viewer rejected`).
- Close the viewer → daemon logs the child exit; tray/daemon stay resident.
- `Ctrl+C` the daemon (or tray → Quit) → viewer is terminated and the socket is
  cleaned up.

## Notes / known limitations

- Icon/`.desktop` association requires the install step; running via
  `cargo run` won't show the branded icon (dev fallback icon instead).
- The window↔`.desktop` match relies on `StartupWMClass=io.github.dipakmdhrm.Fodder`
  (the GTK app ID). If a shell shows a generic window icon, verify that line.
