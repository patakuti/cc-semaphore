// Thin wiring shared by index.html and popup.html: owns the 1s redraw
// timer (02_design.md §10) and re-renders the last snapshot it was given.
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

function render() {
    const sessionsEl = document.getElementById('sessions');
    const countsEl = document.getElementById('counts');
    if (sessionsEl)
        renderPanel(latestSnapshot, sessionsEl, {homeDir, now: Date.now(), daemonAlive});
    if (countsEl && latestSnapshot)
        renderCounts(latestSnapshot.counts, countsEl, {daemonAlive});
}

export function updateSnapshot(snapshot, opts = {}) {
    latestSnapshot = snapshot;
    if (opts.homeDir !== undefined)
        homeDir = opts.homeDir;
    if (opts.daemonAlive !== undefined)
        daemonAlive = opts.daemonAlive;
    render();
}

// Called independently of updateSnapshot when the daemon's liveness
// changes with no accompanying data change (e.g. it just stopped) — see
// ui/tauri-bridge.js's "daemon-status" listener and 02_design.md §3.9.
export function setDaemonAlive(alive) {
    daemonAlive = alive;
    render();
}

setInterval(render, 1000);
render(); // initial paint of the empty state, before any data arrives

window.ccSemaphoreUpdate = updateSnapshot;
window.ccSemaphoreSetDaemonStatus = setDaemonAlive;
