// Thin wiring shared by index.html and popup.html: owns the 1s redraw
// timer (02_design.md §10) and re-renders the last snapshot it was given.
// It does not fetch anything itself — Phase 5/6 (the Tauri desktop crate)
// call `window.ccSemaphoreUpdate(snapshot)` from a Rust `emit` listener;
// demo.html calls it directly with a static sample for browser preview.

import {renderPanel, renderCounts} from './panel.js';

let latestSnapshot = null;
let homeDir = null;

function render() {
    const sessionsEl = document.getElementById('sessions');
    const countsEl = document.getElementById('counts');
    if (sessionsEl)
        renderPanel(latestSnapshot, sessionsEl, {homeDir, now: Date.now()});
    if (countsEl && latestSnapshot)
        renderCounts(latestSnapshot.counts, countsEl);
}

export function updateSnapshot(snapshot, opts = {}) {
    latestSnapshot = snapshot;
    if (opts.homeDir !== undefined)
        homeDir = opts.homeDir;
    render();
}

setInterval(render, 1000);
render(); // initial paint of the empty state, before any data arrives

window.ccSemaphoreUpdate = updateSnapshot;
