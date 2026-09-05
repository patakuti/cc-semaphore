//! Installs a `~/.bashrc` hook that starts `cc-semaphored daemon` in the
//! background on every new interactive shell. See 02_design.md §3.7.
//!
//! WSL1 has no systemd (unlike native Ubuntu, where `systemd::install`
//! handles this), and doesn't run anything on "boot" the way a normal
//! Linux machine does — it only starts when the user opens a shell. A
//! shell-startup hook is the WSL1 equivalent of `WantedBy=default.target`.
//!
//! Defaults to editing `~/.bashrc` directly: appending a clearly delimited,
//! idempotent block to a file the user can immediately read and diff is
//! the same level of directness `systemd::install` already uses when it
//! writes the unit file — the caution that module documents is about not
//! silently invoking `systemctl` (an external command with side effects),
//! not about avoiding file writes altogether. Some users would rather not
//! have a tool touch their shell rc file at all, though (user feedback,
//! 2026-09-05) — `Mode::Print` covers that by only printing the snippet
//! for them to add by hand.

use std::path::{Path, PathBuf};

pub enum Mode {
    /// Append to `~/.bashrc` directly (the default).
    Edit,
    /// Only print the snippet; never touch any file.
    Print,
}

const BEGIN_MARKER: &str = "# cc-semaphore: BEGIN (WSL1 autostart)";
const END_MARKER: &str = "# cc-semaphore: END";

fn bashrc_path() -> PathBuf {
    crate::env::home_dir().join(".bashrc")
}

fn already_installed(bashrc: &str) -> bool {
    bashrc.contains(BEGIN_MARKER)
}

fn build_snippet(exe: &Path) -> String {
    format!(
        "{BEGIN_MARKER}\n\
         if [[ $- == *i* ]]; then\n\
         \x20\x20\x20\x20{} daemon >/dev/null 2>&1 &\n\
         \x20\x20\x20\x20disown\n\
         fi\n\
         {END_MARKER}\n",
        exe.display()
    )
}

/// Appends `snippet` to `bashrc`, separated by a single blank line (unless
/// `bashrc` is empty or already ends with one).
fn appended(bashrc: &str, snippet: &str) -> String {
    if bashrc.is_empty() {
        snippet.to_string()
    } else if bashrc.ends_with("\n\n") {
        format!("{bashrc}{snippet}")
    } else if bashrc.ends_with('\n') {
        format!("{bashrc}\n{snippet}")
    } else {
        format!("{bashrc}\n\n{snippet}")
    }
}

pub fn install(mode: Mode) -> Result<(), String> {
    if !crate::env::is_wsl() {
        eprintln!(
            "cc-semaphored: this doesn't look like WSL1 — on native Linux, \
             use `install-service` instead. Continuing anyway."
        );
    }

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let snippet = build_snippet(&exe);

    if matches!(mode, Mode::Print) {
        println!("Add this block to ~/.bashrc (or ~/.zshrc) yourself:\n");
        print!("{snippet}");
        return Ok(());
    }

    let path = bashrc_path();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if already_installed(&existing) {
        println!(
            "{} already has the autostart hook installed.",
            path.display()
        );
        return Ok(());
    }

    let updated = appended(&existing, &snippet);
    std::fs::write(&path, updated).map_err(|e| e.to_string())?;

    println!("Added an autostart hook to {}.", path.display());
    println!("Open a new shell (or `source ~/.bashrc`) to start the daemon.");
    println!("(Using zsh instead? Add the same block to ~/.zshrc by hand.)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_an_existing_install() {
        let bashrc = format!("stuff\n\n{}\n...\n{END_MARKER}\n", BEGIN_MARKER);
        assert!(already_installed(&bashrc));
    }

    #[test]
    fn does_not_flag_a_bashrc_without_the_hook() {
        assert!(!already_installed("export PATH=$PATH:/usr/local/bin\n"));
    }

    #[test]
    fn snippet_backgrounds_the_daemon_only_when_interactive() {
        let snippet = build_snippet(Path::new("/usr/local/bin/cc-semaphored"));
        assert!(snippet.contains("if [[ $- == *i* ]]; then"));
        assert!(snippet.contains("/usr/local/bin/cc-semaphored daemon >/dev/null 2>&1 &"));
        assert!(snippet.contains("disown"));
    }

    #[test]
    fn appends_after_a_blank_line_when_bashrc_has_content() {
        let result = appended("existing line\n", "NEW\n");
        assert_eq!(result, "existing line\n\nNEW\n");
    }

    #[test]
    fn does_not_add_an_extra_blank_line_when_one_already_trails() {
        let result = appended("existing line\n\n", "NEW\n");
        assert_eq!(result, "existing line\n\nNEW\n");
    }

    #[test]
    fn writes_just_the_snippet_into_an_empty_bashrc() {
        assert_eq!(appended("", "NEW\n"), "NEW\n");
    }
}
