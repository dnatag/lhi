use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;

use crate::watcher;

/// Starts watching a directory for file changes (blocking).
pub fn watch(path: &Path, verbose: bool) -> Result<()> {
    let mut w = watcher::LhiWatcher::new(path)?;
    let canon = path.canonicalize()?;
    eprintln!("lhi: watching {}", canon.display());

    while let Some(event) = w.next_event() {
        let json = serde_json::to_string(&event)?;
        if verbose {
            println!("{json}");
        }
    }
    Ok(())
}

/// Spawns the watcher as a background daemon.
/// Re-execs the current binary with `watch` (no --daemon) and redirects
/// stdout/stderr to `.lhi/watch.log`. Writes PID to `.lhi/watch.pid`.
pub fn watch_daemon(path: &Path, verbose: bool) -> Result<()> {
    let canon = path.canonicalize()?;
    let lhi_dir = canon.join(".lhi");
    fs::create_dir_all(&lhi_dir)?;

    let pid_path = lhi_dir.join("watch.pid");
    if let Some(pid) = read_alive_pid(&pid_path) {
        bail!("Watcher already running (PID {pid}). Use `lhi watch stop` first.");
    }

    let exe = std::env::current_exe().context("cannot find lhi binary")?;
    let log_file = fs::File::create(lhi_dir.join("watch.log"))
        .context("cannot create watch.log")?;
    let log_err = log_file.try_clone()?;

    let mut cmd = std::process::Command::new(exe);
    cmd.arg("watch");
    if verbose { cmd.arg("--verbose"); }
    cmd.arg(&canon);
    cmd.stdout(log_file);
    cmd.stderr(log_err);
    cmd.stdin(std::process::Stdio::null());

    let child = cmd.spawn().context("failed to spawn daemon")?;
    let pid = child.id();
    fs::write(&pid_path, pid.to_string())?;

    eprintln!("lhi: watcher started (PID {pid})");
    eprintln!("lhi: log at {}", lhi_dir.join("watch.log").display());
    Ok(())
}

/// Stops the background watcher daemon.
pub fn watch_stop(path: &Path) -> Result<()> {
    let canon = path.canonicalize()?;
    let pid_path = canon.join(".lhi/watch.pid");

    let Some(pid) = read_pid(&pid_path)? else {
        bail!("No watcher running (no PID file).");
    };

    #[cfg(unix)]
    {
        use std::time::{Duration, Instant};
        // Send SIGTERM
        unsafe { libc::kill(pid as i32, libc::SIGTERM); }

        // Wait up to 3 seconds for process to exit
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if !is_process_alive(pid) { break; }
            std::thread::sleep(Duration::from_millis(100));
        }

        if is_process_alive(pid) {
            unsafe { libc::kill(pid as i32, libc::SIGKILL); }
        }
    }

    #[cfg(not(unix))]
    {
        bail!("Daemon stop is only supported on Unix systems. Kill PID {pid} manually.");
    }

    let _ = fs::remove_file(&pid_path);
    eprintln!("lhi: watcher stopped (PID {pid})");
    Ok(())
}

/// Checks if the background watcher daemon is running.
pub fn watch_status(path: &Path) -> Result<()> {
    let canon = path.canonicalize()?;
    let pid_path = canon.join(".lhi/watch.pid");

    match read_alive_pid(&pid_path) {
        Some(pid) => eprintln!("lhi: watcher running (PID {pid})"),
        None => eprintln!("lhi: watcher not running"),
    }
    Ok(())
}

/// Reads PID from file, returns None if file doesn't exist.
fn read_pid(pid_path: &Path) -> Result<Option<u32>> {
    match fs::read_to_string(pid_path) {
        Ok(s) => Ok(Some(s.trim().parse().context("invalid PID file")?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Reads PID from file and returns it only if the process is still alive.
fn read_alive_pid(pid_path: &Path) -> Option<u32> {
    let pid = read_pid(pid_path).ok()??;
    if is_process_alive(pid) {
        Some(pid)
    } else {
        let _ = fs::remove_file(pid_path); // stale PID file
        None
    }
}

/// Checks if a process with the given PID is alive.
fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    { unsafe { libc::kill(pid as i32, 0) == 0 } }
    #[cfg(not(unix))]
    { false }
}
