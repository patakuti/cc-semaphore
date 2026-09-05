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
const FALLBACK_POLL_SECS = 30;
const MENU_REDRAW_SECS = 1;

// How long a session stays "recent" (blink-worthy) after its `since`.
// Mirrors tray_state.rs's ALERT_WINDOW_MS exactly (02_design.md §6.3).
const ALERT_WINDOW_MS = 30000;

// Must match cc_semaphore_core::heartbeat::STALE_AFTER_MS (02_design.md §3.9).
const STALE_AFTER_MS = 15000;
// Independent of FALLBACK_POLL_SECS: state.json isn't rewritten when nothing
// changed, so heartbeat staleness must be checked on its own cadence rather
// than piggybacking on the FileMonitor/fallback-poll refresh. 5s matches the
// native Linux daemon's own scan tick, giving 3x margin under the threshold.
const HEARTBEAT_POLL_SECS = 5;

// See 02_design.md §2.1: same resolution the daemon uses for the local
// snapshot target.
function statePath() {
    const runtimeDir = GLib.getenv('XDG_RUNTIME_DIR');
    if (runtimeDir)
        return GLib.build_filenamev([runtimeDir, 'cc-semaphore', 'state.json']);
    return GLib.build_filenamev([GLib.get_home_dir(), '.cache', 'cc-semaphore', 'state.json']);
}

// See 02_design.md §3.9: sibling of state.json, rewritten unconditionally by
// the daemon on every scan tick regardless of content.
function heartbeatPath() {
    return GLib.build_filenamev([GLib.path_get_dirname(statePath()), 'heartbeat.json']);
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

// Mirrors cc_semaphore_core::heartbeat::daemon_alive: fresh mtime on
// heartbeat.json means the daemon is alive; missing/unreadable means dead.
function daemonAlive() {
    try {
        const file = Gio.File.new_for_path(heartbeatPath());
        const info = file.query_info('time::modified', Gio.FileQueryInfoFlags.NONE, null);
        const mtimeMs = info.get_attribute_uint64('time::modified') * 1000;
        const nowMs = GLib.get_real_time() / 1000;
        return nowMs - mtimeMs < STALE_AFTER_MS;
    } catch (e) {
        return false;
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

// Whether any session in `sessions` is in `state` and entered it within
// ALERT_WINDOW_MS of `now`. Deliberately holds no alert-episode state of its
// own (no fired-at timestamps, no baselines): recomputed fresh from each
// session's own `since` every call. Mirrors tray_state.rs's `has_recent`
// exactly — see that module's doc comment for why this design (rather than
// tracking count deltas) correctly handles every edge case found in
// 02_design.md §6.3's revision history.
function hasRecent(sessions, state, now) {
    return sessions.some(s => s.state === state && now - s.since < ALERT_WINDOW_MS);
}

// What the panel should show right now: `{kind, visible}`, where `kind` is
// 'waiting', 'idle', or null for steady rotation (no blink), and `visible`
// is the current blink phase (always true when `kind` is null).
function blinkDisplay(sessions, now) {
    // waiting takes priority over idle when both have a recent session,
    // per 02_design.md §6.3.
    let kind = null;
    if (hasRecent(sessions, 'waiting', now))
        kind = 'waiting';
    else if (hasRecent(sessions, 'idle', now))
        kind = 'idle';
    const visible = kind === null || now % (BLINK_INTERVAL_MS * 2) < BLINK_INTERVAL_MS;
    return {kind, visible};
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
        this._daemonDownLabel = new St.Label({
            y_align: Clutter.ActorAlign.CENTER,
            style_class: 'ccs-daemon-down',
            text: '⚠',
            visible: false,
        });
        box.add_child(this._runningLabel);
        box.add_child(this._waitingLabel);
        box.add_child(this._idleLabel);
        box.add_child(this._daemonDownLabel);
        this.add_child(box);

        this._sessionSection = new PopupMenu.PopupMenuSection();
        this.menu.addMenuItem(this._sessionSection);

        this._lastSnapshot = null;
        this._daemonAlive = true;
        this._fileMonitor = null;
        this._pollTimerId = null;
        this._heartbeatTimerId = null;
        this._menuTimerId = null;
        this._blinkTimerId = null;

        this.menu.connect('open-state-changed', (_menu, open) => {
            if (open) {
                this._refresh();
                this._startMenuTimer();
            } else {
                this._stopMenuTimer();
            }
        });

        this._startWatching();
        this._startBlinking();
        this._refresh();
    }

    // Runs for the indicator's entire lifetime (unlike the file-monitor and
    // heartbeat timers, this doesn't need to react to any particular event
    // — it just recomputes the blink display fresh from the latest snapshot
    // on every tick).
    _startBlinking() {
        this._blinkTimerId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, BLINK_INTERVAL_MS, () => {
            const sessions = this._lastSnapshot?.sessions ?? [];
            const {kind, visible} = blinkDisplay(sessions, Date.now());
            this._applyBlink(kind, visible);
            return GLib.SOURCE_CONTINUE;
        });
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
        // Independent daemon-liveness tick per 02_design.md §3.9: state.json
        // doesn't change (so the FileMonitor above stays silent) merely
        // because the daemon died, so staleness needs its own poll.
        this._heartbeatTimerId = GLib.timeout_add_seconds(
            GLib.PRIORITY_DEFAULT, HEARTBEAT_POLL_SECS, () => {
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
        this._daemonAlive = daemonAlive();
        this._renderPanel();
        // Always keep the menu populated, not just while open: PopupMenu's
        // open() refuses to open an empty menu (isEmpty() guard), so if we
        // only filled it in on 'open-state-changed', the menu would never
        // have content when the user first clicks and could never open.
        this._renderMenu();
    }

    _renderPanel() {
        if (!this._daemonAlive) {
            this._runningLabel.hide();
            this._waitingLabel.hide();
            this._idleLabel.hide();
            this._daemonDownLabel.show();
            return;
        }
        this._daemonDownLabel.hide();
        this._runningLabel.show();
        this._waitingLabel.show();
        this._idleLabel.show();
        const counts = this._lastSnapshot?.counts ?? null;
        this._setCount(this._runningLabel, counts?.running);
        this._setCount(this._waitingLabel, counts?.waiting);
        this._setCount(this._idleLabel, counts?.idle);
    }

    _setCount(label, value) {
        label.text = value === undefined ? '-' : `${value}`;
    }

    _applyBlink(kind, visible) {
        // Set Clutter's `opacity` actor property directly rather than
        // toggling a CSS class (`ccs-blink-hidden`, now removed): the
        // latter reliably updated `style_class` but St's theme engine never
        // actually recomputed the rendered opacity from it in testing, so
        // the class was toggling with no visible effect.
        const labels = {waiting: this._waitingLabel, idle: this._idleLabel};
        for (const [k, label] of Object.entries(labels))
            label.opacity = kind === k && !visible ? 0 : 255;
    }

    _renderMenu() {
        this._sessionSection.removeAll();
        if (!this._daemonAlive) {
            const item = new PopupMenu.PopupMenuItem('⚠ daemon not running', {reactive: false});
            item.add_style_class_name('ccs-popup-daemon-down');
            this._sessionSection.addMenuItem(item);
            return;
        }
        const sessions = this._lastSnapshot?.sessions ?? [];
        if (sessions.length === 0) {
            const item = new PopupMenu.PopupMenuItem('No Claude Code sessions', {reactive: false});
            item.add_style_class_name('ccs-popup-empty');
            this._sessionSection.addMenuItem(item);
            return;
        }

        // Ascending elapsed-in-state (most recently changed on top),
        // ignoring the waiting→idle→running priority grouping the backend
        // itself produces — matches the always-on-top window's identical
        // override in ui/app.js's sessionsForList() (user request
        // 2026-09-05: the two session-list UIs should agree). Display-only:
        // never mutates this._lastSnapshot.
        const sorted = [...sessions].sort((a, b) => b.since - a.since);

        const now = Date.now();
        for (const s of sorted) {
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
        if (this._blinkTimerId) {
            GLib.source_remove(this._blinkTimerId);
            this._blinkTimerId = null;
        }
        if (this._fileMonitor) {
            this._fileMonitor.cancel();
            this._fileMonitor = null;
        }
        if (this._pollTimerId) {
            GLib.source_remove(this._pollTimerId);
            this._pollTimerId = null;
        }
        if (this._heartbeatTimerId) {
            GLib.source_remove(this._heartbeatTimerId);
            this._heartbeatTimerId = null;
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
