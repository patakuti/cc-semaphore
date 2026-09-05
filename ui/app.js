// Thin wiring for index.html: owns the 1s redraw timer (02_design.md §10)
// and re-renders the last snapshot it was given.
// It does not fetch anything itself — Phase 5/6 (the Tauri desktop crate)
// call `window.ccSemaphoreUpdate(snapshot)` from a Rust `emit` listener;
// demo.html calls it directly with a static sample for browser preview.

import {renderPanel, renderCounts} from './panel.js';

let latestSnapshot = null;
let homeDir = null;
// Optimistic default: don't flash a warning before the first real status
// (from get_snapshot's response or the first "snapshot"/"daemon-status"
// event) has arrived (02_design.md §3.9).
let daemonAlive = true;
// Same optimistic-default reasoning as daemonAlive above, and likewise
// only ever set to false by real data (02_design.md §2.2.1) — this build
// can't know a snapshot is unsupported until it has actually seen one.
let versionSupported = true;

function render() {
    const sessionsEl = document.getElementById('sessions');
    const countsEl = document.getElementById('counts');
    if (sessionsEl)
        renderPanel(sessionsForList(latestSnapshot), sessionsEl, {homeDir, now: Date.now(), daemonAlive, versionSupported});
    if (countsEl && latestSnapshot)
        renderCounts(latestSnapshot.counts, countsEl, {daemonAlive, versionSupported});
    fitWindowToCard();
}

// The expanded list orders sessions by ascending elapsed-in-state (most
// recently changed on top), ignoring the waiting→idle→running priority
// grouping the backend itself produces (02_design.md §2.4, §7.2). This
// reorder is display-only — it never touches the snapshot the backend
// produced, and panel.js's own rendering still just renders whatever
// array it's handed, unsorted. The GNOME extension applies the identical
// rule independently in its own rendering code (extension.js's
// `_renderMenu`, 2026-09-05 — it doesn't share this JS module), so both
// of the project's two session-list UIs now agree; the backend's own
// order remains its documented contract for any other consumer
// (docs/protocol.md).
function sessionsForList(snapshot) {
    if (!snapshot || !document.body.classList.contains('ccs-window'))
        return snapshot;
    const sessions = [...snapshot.sessions].sort((a, b) => b.since - a.since);
    return {...snapshot, sessions};
}

// Always-on-top window only (02_design.md §7.2, 2026-09-03): resizes the
// actual OS window — not just the visible card — to match its current
// content (collapsed counts-only, or expanded with the session list).
// Without this the window's own footprint stays at whatever size it last
// was, an invisible dead zone beyond the card that still intercepts
// clicks/drags meant for whatever's beneath it. Called from render()
// (i.e. on every real data update, not just on collapse/expand toggles)
// so this also naturally keeps the expanded view fitted as the session
// list grows or shrinks. `window.__TAURI__` is absent in demo.html's
// plain-browser preview, so this is a no-op there.
//
// Deliberately NOT called eagerly at script load, before any real
// snapshot has arrived: measured live that doing so raced the counts
// row's first paint (still empty at that point) and asked the OS window
// to fit an empty card — the window then stayed stuck at that padding-only
// size (~24x40) even once real data and its correct measurement arrived
// moments later, since GTK/WebKitGTK apparently only auto-fits a
// newly-shown window's size once, not continuously (02_design.md §7.2).
function fitWindowToCard() {
    const tauriWindow = window.__TAURI__?.window;
    const card = document.querySelector('.ccs-card');
    if (!tauriWindow || !card || !document.body.classList.contains('ccs-window'))
        return;
    const {width, height} = card.getBoundingClientRect();
    const win = tauriWindow.getCurrentWindow();
    // PhysicalSize computed from the webview's own devicePixelRatio, not
    // LogicalSize (which Tauri converts using its Rust-side scaleFactor()):
    // measured on Windows/WebView2 that the two can disagree (scaleFactor()
    // 1.25 vs devicePixelRatio 1.5, likely a stale value from before the
    // window — created hidden — was actually shown on its real monitor),
    // undersizing the window relative to what the webview itself renders
    // (02_design.md §7.2, user-reported 2026-09-05). devicePixelRatio is
    // always the webview's own live value, so this can't disagree with
    // itself the way cross-checking against Tauri's separate value can.
    const dpr = window.devicePixelRatio || 1;
    win.setSize(new tauriWindow.PhysicalSize(
        Math.ceil(width * dpr), Math.ceil(height * dpr)));
}

export function updateSnapshot(snapshot, opts = {}) {
    latestSnapshot = snapshot;
    if (opts.homeDir !== undefined)
        homeDir = opts.homeDir;
    if (opts.daemonAlive !== undefined)
        daemonAlive = opts.daemonAlive;
    if (opts.versionSupported !== undefined)
        versionSupported = opts.versionSupported;
    render();
}

// Called independently of updateSnapshot when the daemon's liveness
// changes with no accompanying data change (e.g. it just stopped) — see
// ui/tauri-bridge.js's "daemon-status" listener and 02_design.md §3.9.
export function setDaemonAlive(alive) {
    daemonAlive = alive;
    render();
}

window.ccSemaphoreUpdate = updateSnapshot;
window.ccSemaphoreSetDaemonStatus = setDaemonAlive;

// Collapse/expand (02_design.md §7.2, 2026-09-03): the always-on-top
// window (.ccs-window) is the only UI this applies to. Collapsed is the
// default: this window sits on top of everything,
// so it should stay out of the way until asked for detail. This is a
// webview-local toggle, independent of the window's own show/hide (driven
// from the Rust side via the tray menu) — the window isn't recreated on
// hide/show, so this state simply persists across that for free.
//
// Applied before the first render() call below, so that call's own
// fitWindowToCard() (triggered from render(), not here) already measures
// the collapsed layout rather than briefly measuring the expanded one.
const countsEl = document.getElementById('counts');
if (countsEl && document.body.classList.contains('ccs-window')) {
    document.body.classList.add('ccs-collapsed');
    countsEl.addEventListener('click', () => {
        document.body.classList.toggle('ccs-collapsed');
        fitWindowToCard();
    });
}

// Card background transparency (02_design.md §7.2, user request
// 2026-09-03): the count circles above are always fully opaque, but the
// rest of the card defaults to 90% transparent (--ccs-card-alpha: 0.1 in
// panel.css) and can be stepped with [ (more transparent) / ] (more
// opaque) while the panel is expanded — collapsed is just a glance-only
// counts row, not something to tune. Persisted in this webview's own
// localStorage (survives app restarts; Tauri gives each app a persistent
// per-app webview data directory) rather than plumbed through a Rust
// command, since the value is only ever read and written from here.
const CARD_ALPHA_KEY = 'ccs-card-alpha';
const CARD_ALPHA_DEFAULT = 0.1;
const CARD_ALPHA_STEP = 0.05;

function loadCardAlpha() {
    const stored = parseFloat(localStorage.getItem(CARD_ALPHA_KEY));
    return Number.isFinite(stored) ? Math.min(1, Math.max(0, stored)) : CARD_ALPHA_DEFAULT;
}

function applyCardAlpha(alpha) {
    document.documentElement.style.setProperty('--ccs-card-alpha', alpha.toFixed(2));
}

if (document.body.classList.contains('ccs-window')) {
    applyCardAlpha(loadCardAlpha());
    document.addEventListener('keydown', (event) => {
        if (document.body.classList.contains('ccs-collapsed'))
            return;
        if (event.key !== '[' && event.key !== ']')
            return;
        event.preventDefault();
        const delta = event.key === ']' ? CARD_ALPHA_STEP : -CARD_ALPHA_STEP;
        const next = Math.min(1, Math.max(0, loadCardAlpha() + delta));
        applyCardAlpha(next);
        localStorage.setItem(CARD_ALPHA_KEY, next.toFixed(2));
    });
}

setInterval(render, 1000);
render(); // initial paint of the empty state, before any data arrives
