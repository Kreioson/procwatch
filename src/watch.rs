use std::io::{self, Write};
use std::time::Duration;

use crate::format;
use crate::storage;

/// Live-refresh mode — clears screen and re-prints every `interval`.
pub fn run_watch(interval_secs: u64, sort_by: &str) {
    let interval = Duration::from_secs(interval_secs);
    let top_n = 15;
    let daemon_running = storage::daemon_is_running();

    // Keep sysinfo alive across cycles so CPU deltas accumulate naturally
    let mut sys = sysinfo::System::new();

    // First refresh establishes baseline
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    std::thread::sleep(Duration::from_millis(500));
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    loop {
        // ── Live snapshot using persistent sysinfo ──
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

        let mut processes: Vec<crate::snapshot::ProcessInfo> = sys
            .processes()
            .iter()
            .map(|(_, p)| {
                let mem_bytes = p.memory();
                let disk = p.disk_usage();
                let cpu = p.cpu_usage();
                let cpu_time = p.accumulated_cpu_time();
                crate::snapshot::ProcessInfo {
                    pid: p.pid().as_u32(),
                    name: p.name().to_string_lossy().into_owned(),
                    cpu_pct: cpu,
                    mem_mb: mem_bytes as f64 / 1_048_576.0,
                    cpu_ms: cpu_time,
                    disk_r: disk.total_read_bytes,
                    disk_w: disk.total_written_bytes,
                }
            })
            .filter(|p| p.cpu_pct > 0.0 || p.mem_mb > 0.0)
            .collect();

        processes.sort_by(|a, b| {
            match sort_by {
                "mem" | "memory" => b.mem_mb
                    .partial_cmp(&a.mem_mb)
                    .unwrap_or(std::cmp::Ordering::Equal),
                "disk" => {
                    let a_total = a.disk_r + a.disk_w;
                    let b_total = b.disk_r + b.disk_w;
                    b_total.partial_cmp(&a_total)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }
                _ => b.cpu_pct
                    .partial_cmp(&a.cpu_pct)
                    .unwrap_or(std::cmp::Ordering::Equal),
            }
        });
        processes.truncate(top_n);

        // ── History from CSV ──
        let history = if daemon_running {
            let since = storage::now_unix() - 12 * 3600;
            let all = storage::read_all().unwrap_or_default();
            storage::aggregate_by_pid(&all, since)
        } else {
            Vec::new()
        };

        // ── Render ──
        print!("\x1b[2J\x1b[H");
        format::print_snapshot(&processes, &history, daemon_running);

        let _ = writeln!(
            io::stdout(),
            " {}refreshing every {interval_secs}s  ·  Ctrl+C to quit{}",
            crate::format::ansi::DIM,
            crate::format::ansi::RESET,
        );

        std::thread::sleep(interval);
    }
}
