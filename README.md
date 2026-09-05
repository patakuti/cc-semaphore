# cc-semaphore

A monitor for people running several [Claude Code](https://claude.com/claude-code)
sessions at once. It shows, at a glance, which of your sessions are
**running**, **waiting** for input, or sitting **idle** after finishing —
so you know which one actually needs your attention.

Supported platforms: Ubuntu (native Linux), and Windows 10/11 with WSL1.

## Screenshots

GNOME Shell top bar (running / waiting / idle counts):

![GNOME top bar](docs/screenshots/gnome-topbar.png)

The always-on-top panel (Windows), collapsed and expanded:

![Panel collapsed](docs/screenshots/panel-collapsed.png)
![Panel expanded](docs/screenshots/panel-expanded.png)

## Status model

| State | Meaning | Color |
|---|---|---|
| `running` | Actively working | Green |
| `waiting` | Waiting on input/approval (needs you) | Yellow |
| `idle` | Finished, sitting untouched (not urgent) | Red |

## Downloads

Prebuilt binaries for every tagged version are attached to the
[GitHub Releases](https://github.com/patakuti/cc-semaphore/releases) page:

- **Linux / WSL1**: `cc-semaphored-linux-x86_64` — a static binary, no
  Rust toolchain needed, works unmodified on native Ubuntu and WSL1.
- **Windows**: either `cc-semaphore-desktop.exe` (just copy and run) or
  the NSIS installer (`cc-semaphore-desktop_*_x64-setup.exe`), which also
  registers the app to start on login.

No release yet, or want the latest unreleased build? Every pull request
also produces the same binaries as workflow artifacts
(`.github/workflows/linux-build.yml` / `windows-build.yml`).

## Getting started

### 1. Install the daemon (`cc-semaphored`)

`cc-semaphored` watches your Claude Code sessions and publishes a snapshot
that the GNOME extension and the desktop app both read. It writes to
`$XDG_RUNTIME_DIR/cc-semaphore/state.json` (falling back to
`~/.cache/cc-semaphore/state.json`); the JSON format itself is documented
in `docs/protocol.md`.

Download `cc-semaphored-linux-x86_64` from
[Releases](https://github.com/patakuti/cc-semaphore/releases) (see
[Downloads](#downloads) above) — no Rust toolchain required, and the same
binary works on native Ubuntu and WSL1. **Recommended install location:
`~/.local/bin/cc-semaphored`** (the autostart hook below embeds whatever
path you run it from, so moving the binary later breaks autostart):

```sh
mkdir -p ~/.local/bin
mv cc-semaphored-linux-x86_64 ~/.local/bin/cc-semaphored
chmod +x ~/.local/bin/cc-semaphored
```

```sh
cc-semaphored once                    # scan once, print the snapshot JSON to stdout
cc-semaphored watch                   # live view in the terminal (redraws every second)
cc-semaphored daemon                  # run as a background daemon
cc-semaphored install-service         # (native Linux) install a systemd user unit
cc-semaphored install-wsl1-autostart  # (WSL1) add an autostart hook to ~/.bashrc
cc-semaphored install-wsl1-autostart --print  # print the hook instead of editing the file
```

Keeping it running differs by platform. On native Linux, `install-service`
sets up a systemd user unit that starts automatically after login. WSL1 has
no systemd and no real equivalent of "OS boot", so
`install-wsl1-autostart` instead adds a hook to `~/.bashrc`: every new
shell tries to start the daemon, and does nothing if one is already running
(enforced by a lock file). If you'd rather not have a tool edit your shell
rc file automatically, add `--print` — it leaves the file untouched and
just prints the block for you to paste in yourself (into `.zshrc`, etc.).

### 2. Pick a frontend

- **GNOME Shell** (Ubuntu): see [GNOME Shell extension](#gnome-shell-extension) below.
- **Windows**: download `cc-semaphore-desktop` (see below) for a system
  tray icon plus an optional always-on-top panel.

## GNOME Shell extension

```sh
ln -sfn "$(pwd)/extensions/cc-semaphore@patakuti" \
  ~/.local/share/gnome-shell/extensions/cc-semaphore@patakuti
# Restart GNOME Shell (X11: Alt+F2 → r → Enter. On Wayland, log out and back in)
gnome-extensions enable cc-semaphore@patakuti
```

The top bar shows the three counts (green/yellow/red). Click it to open a
popup with the full session list, sorted so the most recently changed
session is at the top. If the daemon isn't running (no heartbeat for 15+
seconds), the counts are replaced by a single `⚠` and the popup says so
too.

## Windows: system tray + always-on-top panel

Easiest path: install via the NSIS installer or just run the `.exe` from
[Releases](https://github.com/patakuti/cc-semaphore/releases) (see
[Downloads](#downloads) above) — either way, running it registers it to
start on login automatically (no opt-out toggle; there's no separate
"install" step beyond running it once).

To run from source instead:

```sh
cc-semaphored daemon &                        # run the daemon separately
cargo run -p cc-semaphore-desktop             # tray icon only, panel hidden by default
cargo run -p cc-semaphore-desktop -- --panel  # also open the panel right away
```

Either mouse button on the tray icon opens the same menu (`Show panel` /
`Quit`). The panel itself starts collapsed, showing just the three counts
as solid badges; click it to expand into the full session list (sorted the
same way as the GNOME extension's popup — most recently changed on top).
The card's background defaults to 90% transparent; while expanded, `[` /
`]` step it in 5% increments, and your choice is remembered across
restarts. If the daemon isn't running, the tray icon turns into a gray `?`
and the panel shows `⚠ daemon not running`.

## Configuration

All of it is optional — cc-semaphore works out of the box with no config
file. To override the defaults, create
`~/.config/cc-semaphore/config.json` (`%APPDATA%\cc-semaphore\config.json`
on Windows):

| Key | Default | Meaning |
|---|---|---|
| `includeKinds` | `["interactive"]` | Which session kinds to show |
| `inotifyDebounceMs` | `150` | Debounce for Linux inotify events |
| `livenessTickSecs` | `5` | How often to re-check process liveness on Linux |
| `wsl1PollIntervalMs` | `1000` | Poll interval on WSL1 |
| `windowsStateDir` | (auto-detected) | Explicit override for the WSL1→Windows snapshot path |
| `windowsPollIntervalMs` | `500` | (`cc-semaphore-desktop` only) mtime poll interval across DrvFs |

Everything else (UI redraw rate, tray icon rotation/blink timing, the
GNOME extension's fallback poll) is a fixed internal constant and isn't
configurable.

## Building from source

```sh
cargo build
cargo test
```

- `crates/cc-semaphore-core` — shared library: session-file parsing,
  the running/waiting/idle mapping, liveness checks, snapshot generation
- `crates/cc-semaphore-daemon` — the `cc-semaphored` binary (inotify
  watcher, WSL1 poller, snapshot writer, CLI)
- `crates/cc-semaphore-desktop` — the Tauri v2 app: always-on-top panel
  and Windows system tray
- `extensions/cc-semaphore@patakuti/` — the GNOME Shell 46 extension
- `ui/` — static HTML/CSS/JS shared by every frontend except the GNOME
  extension (no build step; no npm/Node.js). Open `ui/demo.html` via
  `python3 -m http.server -d ui` for a standalone preview with sample data
- `docs/protocol.md` — the JSON snapshot format shared between the daemon
  and every frontend

No npm, no bundler: `ui/`'s plain ES modules are loaded directly as
Tauri's `frontendDist`.
