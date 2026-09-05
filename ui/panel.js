// Pure session-list rendering. No build step, no framework — a plain ES
// module consumed directly by index.html (the always-on-top window). See
// 02_design.md §4.2, §7.3.
//
// This module never fetches or times anything itself: callers (app.js)
// own the data source and the redraw timer, and pass in `now` so this
// stays a pure function of its inputs.

// Must match crates/cc-semaphore-core/src/colors.rs (02_design.md §4.1).
export const COLORS = {
    running: '#2ec27e',
    waiting: '#f5c211',
    idle: '#e01b24',
};

// Mirrors crates/cc-semaphore-core/src/format.rs::format_elapsed exactly.
export function formatElapsed(seconds) {
    seconds = Math.max(0, Math.floor(seconds));
    if (seconds < 60)
        return `${seconds}s`;
    if (seconds < 3600)
        return `${Math.floor(seconds / 60)}m${seconds % 60}s`;
    return `${Math.floor(seconds / 3600)}h${Math.floor((seconds % 3600) / 60)}m`;
}

// Replaces the home directory prefix with `~`, then truncates from the
// front (keeping the tail) if still longer than `maxLen`, per 02_design.md
// §4.2 ("長い場合は先頭側を省略(末尾を残す)").
export function shortenCwd(cwd, homeDir, maxLen = 40) {
    let s = cwd;
    if (homeDir && s.startsWith(homeDir))
        s = `~${s.slice(homeDir.length)}`;
    if (s.length > maxLen)
        s = `…${s.slice(s.length - maxLen + 1)}`;
    return s;
}

function el(tag, className, text) {
    const e = document.createElement(tag);
    if (className)
        e.className = className;
    if (text !== undefined)
        e.textContent = text;
    return e;
}

function renderRow(session, homeDir, now) {
    const row = el('div', 'ccs-row');
    row.appendChild(el('span', `ccs-dot ccs-${session.state}`, '●'));
    row.appendChild(el('span', 'ccs-name', session.name || String(session.pid)));
    row.appendChild(el('span', 'ccs-cwd', shortenCwd(session.cwd, homeDir)));

    const elapsedSecs = (now - session.since) / 1000;
    row.appendChild(el('span', 'ccs-elapsed', formatElapsed(elapsedSecs)));

    if (session.state === 'waiting' && session.waitingFor)
        row.appendChild(el('span', 'ccs-waiting-for', `(${session.waitingFor})`));

    return row;
}

// Renders the session list into `rootEl`. `snapshot` may be null/undefined
// (no data yet); `opts.homeDir` enables `~` shortening; `opts.now`
// defaults to the current time and should be passed explicitly by callers
// that redraw on a timer, so a whole batch of rows uses one consistent
// clock reading. `opts.daemonAlive === false` (02_design.md §3.9) shows a
// warning instead of the session list — an empty list and "daemon isn't
// running" are different situations and must not look the same.
export function renderPanel(snapshot, rootEl, opts = {}) {
    const homeDir = opts.homeDir ?? null;
    const now = opts.now ?? Date.now();

    rootEl.textContent = '';
    if (opts.daemonAlive === false) {
        rootEl.appendChild(el('div', 'ccs-daemon-down', '⚠ daemon not running'));
        return;
    }
    const sessions = snapshot?.sessions ?? [];
    if (sessions.length === 0) {
        rootEl.appendChild(el('div', 'ccs-empty', 'No Claude Code sessions'));
        return;
    }
    // This module never sorts — it renders `snapshot.sessions` in
    // whatever order it's given. Sessions arrive pre-sorted by the
    // backend (02_design.md §2.4); the always-on-top window's app.js is
    // the one exception, reordering its own copy before calling in here
    // (§7.2) — that reorder belongs to the caller, not this module.
    for (const session of sessions)
        rootEl.appendChild(renderRow(session, homeDir, now));
}

// Renders the three running/waiting/idle counters into `rootEl`. Cleared
// (not zeroed) when the daemon is down, per the same reasoning as above.
export function renderCounts(counts, rootEl, opts = {}) {
    rootEl.textContent = '';
    if (opts.daemonAlive === false)
        return;
    for (const key of ['running', 'waiting', 'idle']) {
        rootEl.appendChild(el('span', `ccs-count ccs-${key}`, String(counts?.[key] ?? 0)));
    }
}
