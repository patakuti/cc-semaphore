// Tauri-only wiring (02_design.md §7.3): listens for the "snapshot" event
// the Rust side emits whenever the watched state.json changes, and feeds
// it into the same window.ccSemaphoreUpdate hook app.js exposes. Loaded
// only by index.html — demo.html and any plain-browser preview never load
// this file, so it is the only place that touches the Tauri
// global (window.__TAURI__, enabled via app.withGlobalTauri in
// tauri.conf.json since this project uses no npm/bundler to import
// @tauri-apps/api).

function apply(payload) {
    if (!payload)
        return;
    const {snapshot, homeDir, daemonAlive} = payload;
    window.ccSemaphoreUpdate(snapshot, {homeDir, daemonAlive});
}

window.__TAURI__.event.listen('snapshot', (event) => apply(event.payload));

// The daemon going stale isn't itself a state.json change, so nothing
// above re-fires when that happens — the Rust side emits this separately
// whenever daemon liveness flips (02_design.md §3.9, tray.rs's tick loop).
window.__TAURI__.event.listen('daemon-status', (event) => {
    window.ccSemaphoreSetDaemonStatus(event.payload.alive);
});

// Fetch whatever is already on disk once the listeners above are
// attached, so a snapshot written before this page loaded isn't missed
// forever (the Rust side only re-emits on the *next* change; see
// main.rs's get_snapshot command).
window.__TAURI__.core.invoke('get_snapshot').then(apply);
