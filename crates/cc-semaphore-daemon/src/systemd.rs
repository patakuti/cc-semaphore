//! Writes the systemd user unit. See 02_design.md §3.7.
//!
//! Deliberately does not call `systemctl` itself: writing a file the user
//! can inspect first, and printing the exact commands to enable it, is
//! safer than silently touching the user's systemd state.

use std::path::PathBuf;

fn unit_path() -> PathBuf {
    crate::env::home_dir().join(".config/systemd/user/cc-semaphore.service")
}

pub fn install() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let unit = format!(
        "[Unit]\n\
         Description=cc-semaphore session monitor daemon\n\
         \n\
         [Service]\n\
         ExecStart={} daemon\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        exe.display()
    );

    let path = unit_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, unit).map_err(|e| e.to_string())?;

    println!("Wrote {}", path.display());
    println!("To enable and start it now, run:");
    println!("  systemctl --user daemon-reload");
    println!("  systemctl --user enable --now cc-semaphore.service");
    Ok(())
}
