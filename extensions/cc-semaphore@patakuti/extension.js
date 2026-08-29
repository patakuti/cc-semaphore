import GObject from 'gi://GObject';
import St from 'gi://St';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Clutter from 'gi://Clutter';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

// Must match 02_design.md §10.
const BLINK_INTERVAL_MS = 500;
const BLINK_DURATION_MS = 30000;
const FALLBACK_POLL_SECS = 30;
const MENU_REDRAW_SECS = 1;

// See 02_design.md §2.1: same resolution the daemon uses for the local
// snapshot target.
function statePath() {
    const runtimeDir = GLib.getenv('XDG_RUNTIME_DIR');
    if (runtimeDir)
        return GLib.build_filenamev([runtimeDir, 'cc-semaphore', 'state.json']);
    return GLib.build_filenamev([GLib.get_home_dir(), '.cache', 'cc-semaphore', 'state.json']);
}

function readSnapshot() {
    try {
        const file = Gio.File.new_for_path(statePath());
        const [ok, contents] = file.load_contents(null);
        if (!ok)
            return null;
        return JSON.parse(new TextDecoder('utf-8').decode(contents));
    } catch (e) {
        return null;
    }
}

// Mirrors crates/cc-semaphore-core/src/format.rs::format_elapsed exactly.
function formatElapsed(seconds) {
    seconds = Math.max(0, seconds);
    if (seconds < 60)
        return `${seconds}s`;
    if (seconds < 3600)
        return `${Math.floor(seconds / 60)}m${seconds % 60}s`;
    return `${Math.floor(seconds / 3600)}h${Math.floor((seconds % 3600) / 60)}m`;
}

function shortenHome(cwd) {
    const home = GLib.get_home_dir();
    return cwd.startsWith(home) ? `~${cwd.slice(home.length)}` : cwd;
}

// Implements the interrupt-blink state machine from 02_design.md §6.3
// (shared spec with the Windows tray). `onUpdate(kind, visible)` is called
// whenever the blinking display state changes; `kind` is 'waiting', 'idle',
// or null when back to steady rotation.
class BlinkController {
    constructor(onUpdate) {
        this._onUpdate = onUpdate;
        this._prevCounts = null;
        this._activeKind = null;
        this._deadlineMs = 0;
        this._timerId = null;
    }

    // Feeds a new `counts` reading. The very first call only establishes a
    // baseline and never triggers a blink (avoids a false alarm on startup).
    update(counts) {
        if (this._prevCounts) {
            // idle first, waiting second: if both increase in the same
            // update, waiting (processed last) wins, per §6.3.
            for (const kind of ['idle', 'waiting']) {
                if (counts[kind] > this._prevCounts[kind])
                    this._fire(kind);
            }
        }
        this._prevCounts = counts;
    }

    _fire(kind) {
        this._activeKind = kind;
        this._deadlineMs = GLib.get_monotonic_time() / 1000 + BLINK_DURATION_MS;
        this._ensureTimer();
    }

    _ensureTimer() {
        if (this._timerId)
            return;
        let visible = true;
        this._timerId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, BLINK_INTERVAL_MS, () => {
            if (GLib.get_monotonic_time() / 1000 >= this._deadlineMs) {
                this._activeKind = null;
                this._timerId = null;
                this._onUpdate(null, true);
                return GLib.SOURCE_REMOVE;
            }
            visible = !visible;
            this._onUpdate(this._activeKind, visible);
            return GLib.SOURCE_CONTINUE;
        });
    }

    destroy() {
        if (this._timerId) {
            GLib.source_remove(this._timerId);
            this._timerId = null;
        }
    }
}

const Indicator = GObject.registerClass(
class Indicator extends PanelMenu.Button {
    _init() {
        super._init(0.0, 'cc-semaphore');

        const box = new St.BoxLayout({style_class: 'panel-status-menu-box'});
        this._runningLabel = new St.Label({
            y_align: Clutter.ActorAlign.CENTER,
            style_class: 'ccs-count ccs-running',
        });
        this._waitingLabel = new St.Label({
            y_align: Clutter.ActorAlign.CENTER,
            style_class: 'ccs-count ccs-waiting',
        });
        this._idleLabel = new St.Label({
            y_align: Clutter.ActorAlign.CENTER,
            style_class: 'ccs-count ccs-idle',
        });
        box.add_child(this._runningLabel);
        box.add_child(this._waitingLabel);
        box.add_child(this._idleLabel);
        this.add_child(box);

        this._sessionSection = new PopupMenu.PopupMenuSection();
        this.menu.addMenuItem(this._sessionSection);

        this._lastSnapshot = null;
        this._fileMonitor = null;
        this._pollTimerId = null;
        this._menuTimerId = null;
        this._blink = new BlinkController((kind, visible) => this._applyBlink(kind, visible));

        this.menu.connect('open-state-changed', (_menu, open) => {
            if (open) {
                this._refresh();
                this._startMenuTimer();
            } else {
                this._stopMenuTimer();
            }
        });

        this._startWatching();
        this._refresh();
    }

    _startWatching() {
        const file = Gio.File.new_for_path(statePath());
        try {
            this._fileMonitor = file.monitor_file(Gio.FileMonitorFlags.NONE, null);
            this._fileMonitor.connect('changed', () => this._refresh());
        } catch (e) {
            this._fileMonitor = null;
        }
        // Safety net per 02_design.md §5.5: covers a monitor that failed to
        // set up (e.g. the daemon hasn't created the directory yet).
        this._pollTimerId = GLib.timeout_add_seconds(
            GLib.PRIORITY_DEFAULT, FALLBACK_POLL_SECS, () => {
                this._refresh();
                return GLib.SOURCE_CONTINUE;
            });
    }

    _startMenuTimer() {
        if (this._menuTimerId)
            return;
        this._menuTimerId = GLib.timeout_add_seconds(
            GLib.PRIORITY_DEFAULT, MENU_REDRAW_SECS, () => {
                this._renderMenu();
                return GLib.SOURCE_CONTINUE;
            });
    }

    _stopMenuTimer() {
        if (this._menuTimerId) {
            GLib.source_remove(this._menuTimerId);
            this._menuTimerId = null;
        }
    }

    _refresh() {
        this._lastSnapshot = readSnapshot();
        this._renderPanel();
        // Always keep the menu populated, not just while open: PopupMenu's
        // open() refuses to open an empty menu (isEmpty() guard), so if we
        // only filled it in on 'open-state-changed', the menu would never
        // have content when the user first clicks and could never open.
        this._renderMenu();
        if (this._lastSnapshot)
            this._blink.update(this._lastSnapshot.counts);
    }

    _renderPanel() {
        const counts = this._lastSnapshot?.counts ?? null;
        this._setCount(this._runningLabel, counts?.running);
        this._setCount(this._waitingLabel, counts?.waiting);
        this._setCount(this._idleLabel, counts?.idle);
    }

    _setCount(label, value) {
        label.text = value === undefined ? '-' : `${value}`;
        label.remove_style_class_name('ccs-zero');
        if (value === 0)
            label.add_style_class_name('ccs-zero');
    }

    _applyBlink(kind, visible) {
        const labels = {waiting: this._waitingLabel, idle: this._idleLabel};
        for (const [k, label] of Object.entries(labels)) {
            label.remove_style_class_name('ccs-blink-hidden');
            if (kind === k && !visible)
                label.add_style_class_name('ccs-blink-hidden');
        }
    }

    _renderMenu() {
        this._sessionSection.removeAll();
        const sessions = this._lastSnapshot?.sessions ?? [];
        if (sessions.length === 0) {
            const item = new PopupMenu.PopupMenuItem('No Claude Code sessions', {reactive: false});
            item.add_style_class_name('ccs-popup-empty');
            this._sessionSection.addMenuItem(item);
            return;
        }

        const now = Date.now();
        for (const s of sessions) {
            const item = new PopupMenu.PopupBaseMenuItem({reactive: false, can_focus: false});
            const dot = new St.Label({text: '●', style_class: `ccs-popup-dot ccs-${s.state}`});
            const nameLabel = new St.Label({text: s.name || String(s.pid)});
            const cwdLabel = new St.Label({text: shortenHome(s.cwd), style_class: 'ccs-popup-cwd'});
            const elapsed = formatElapsed(Math.floor((now - s.since) / 1000));
            const suffix = s.waitingFor ? ` (${s.waitingFor})` : '';
            const elapsedLabel = new St.Label({
                text: `${elapsed}${suffix}`,
                style_class: 'ccs-popup-elapsed',
            });
            item.add_child(dot);
            item.add_child(nameLabel);
            item.add_child(cwdLabel);
            item.add_child(elapsedLabel);
            this._sessionSection.addMenuItem(item);
        }
    }

    destroy() {
        this._blink.destroy();
        if (this._fileMonitor) {
            this._fileMonitor.cancel();
            this._fileMonitor = null;
        }
        if (this._pollTimerId) {
            GLib.source_remove(this._pollTimerId);
            this._pollTimerId = null;
        }
        this._stopMenuTimer();
        super.destroy();
    }
});

export default class CcSemaphoreExtension extends Extension {
    enable() {
        this._indicator = new Indicator();
        Main.panel.addToStatusArea(this.uuid, this._indicator);
    }

    disable() {
        this._indicator?.destroy();
        this._indicator = null;
    }
}
