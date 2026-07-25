use std::fs;
use std::time::Duration;

use crate::snapshot::ProcessInfo;
use crate::storage::{self, SampleRow};

const DEFAULT_INTERVAL_SECS: u64 = 30;
const PRUNE_AFTER_SECS: i64 = 7 * 86400; // 7 days
const PRUNE_EVERY_N: u32 = 100;
const TOP_N: usize = 15;

/// Start the daemon — self-daemonizes on first call.
pub fn run_daemon(interval_secs: Option<u64>) {
    let interval = interval_secs.unwrap_or(DEFAULT_INTERVAL_SECS);

    if std::env::var("PROCWATCH_DAEMONIZED").is_err() {
        spawn_daemon(interval);
        return;
    }

    // ── Actual daemon process ──
    let data_dir = storage::get_data_dir();
    fs::create_dir_all(&data_dir).ok();
    storage::write_pid(std::process::id()).ok();

    // Cleanup PID file on exit
    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            storage::remove_pid();
        }
    }
    let _cleanup = Cleanup;

    // ── Sampling loop ──
    let mut sys = sysinfo::System::new();
    let mut sample_count: u32 = 0;

    loop {
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        std::thread::sleep(Duration::from_millis(1000));
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

        let now = storage::now_unix();

        let mut procs: Vec<ProcessInfo> = sys
            .processes()
            .iter()
            .map(|(_, p)| {
                let mem = p.memory();
                let disk = p.disk_usage();
                let cpu = p.cpu_usage();
                let cpu_time = p.accumulated_cpu_time();
                ProcessInfo {
                    pid: p.pid().as_u32(),
                    name: p.name().to_string_lossy().into_owned(),
                    cpu_pct: cpu,
                    mem_mb: mem as f64 / 1_048_576.0,
                    cpu_ms: cpu_time,
                    disk_r: disk.total_read_bytes,
                    disk_w: disk.total_written_bytes,
                }
            })
            .filter(|p| p.cpu_pct > 0.0 || p.mem_mb > 0.0)
            .collect();

        procs.sort_by(|a, b| {
            b.cpu_pct
                .partial_cmp(&a.cpu_pct)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        procs.truncate(TOP_N);

        for p in &procs {
            let row = SampleRow {
                ts: now,
                pid: p.pid,
                name: p.name.clone(),
                cpu_pct: p.cpu_pct,
                mem_mb: p.mem_mb,
                cpu_ms: p.cpu_ms,
                disk_r: p.disk_r,
                disk_w: p.disk_w,
            };
            if let Err(e) = storage::append_sample(&row) {
                eprintln!("procwatch: write error: {e}");
            }
        }

        sample_count += 1;
        if sample_count % PRUNE_EVERY_N == 0 {
            let _ = storage::prune_old(storage::now_unix() - PRUNE_AFTER_SECS);
        }

        // Check if PID file still ours
        if storage::read_pid().map(|p| p as usize) != Some(std::process::id() as usize) {
            break;
        }

        std::thread::sleep(Duration::from_secs(interval));
    }
}

/// Spawn the daemon in background and exit the parent.
fn spawn_daemon(interval: u64) {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("procwatch: can't locate binary: {e}");
            std::process::exit(1);
        }
    };

    // On Unix, simply spawn with null IO — child is a separate process.
    // On Windows, use `start /B` to fully detach from the console.
    let child = spawn_background(&exe);

    let child = match child {
        Ok(c) => c,
        Err(e) => {
            eprintln!("procwatch: failed to spawn daemon: {e}");
            std::process::exit(1);
        }
    };

    // Wait for the daemon to write its PID file
    let daemon_pid = (0..30).into_iter().find_map(|_| {
        std::thread::sleep(Duration::from_millis(100));
        storage::read_pid()
    }).unwrap_or(child.id());

    println!("procwatch daemon started (PID {daemon_pid})");
    println!(
        "  sampling every {interval}s   data: {}",
        storage::get_data_dir().display()
    );
    println!("  run `procwatch` to view live data with history");
    println!("  run `procwatch watch` for live-refresh mode");
    println!("  run `procwatch stop` to stop the daemon");
    std::process::exit(0);
}

#[cfg(unix)]
fn spawn_background(exe: &std::path::Path) -> std::io::Result<std::process::Child> {
    std::process::Command::new(exe)
        .arg("daemon")
        .env("PROCWATCH_DAEMONIZED", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
}

#[cfg(windows)]
fn spawn_background(exe: &std::path::Path) -> std::io::Result<std::process::Child> {
    // `start /B` launches the process detached from the console.
    // Closing the terminal won't kill the daemon.
    std::process::Command::new("cmd")
        .args(["/C", "start", "/B", &exe.display().to_string(), "daemon"])
        .env("PROCWATCH_DAEMONIZED", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
}

/// Stop the daemon by killing the process in the PID file.
pub fn stop_daemon() -> bool {
    let pid = match storage::read_pid() {
        Some(p) => p,
        None => {
            eprintln!("procwatch: no daemon running (no PID file)");
            return false;
        }
    };

    #[cfg(unix)]
    {
        let result = std::process::Command::new("kill")
            .arg(pid.to_string())
            .status();
        match result {
            Ok(status) if status.success() => {
                for _ in 0..20 {
                    if !storage::daemon_is_running() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                storage::remove_pid();
                println!("procwatch daemon (PID {pid}) stopped");
                true
            }
            _ => {
                eprintln!("procwatch: could not stop daemon (PID {pid}) — try `kill {pid}`");
                false
            }
        }
    }

    #[cfg(not(unix))]
    {
        eprintln!("procwatch: use `taskkill /PID {pid}` on Windows to stop the daemon");
        false
    }
}
