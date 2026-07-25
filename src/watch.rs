use std::io::{self, Write};
use std::time::Duration;

use crate::format;
use crate::snapshot;
use crate::storage;

/// Live-refresh mode — clears screen and re-prints every `interval`.
pub fn run_watch(interval_secs: u64) {
    let interval = Duration::from_secs(interval_secs);
    let top_n = 15;
    let daemon_running = storage::daemon_is_running();

    // Load history once, then re-use (we could reload every cycle, but for speed we just
    // reload every cycle anyway since history is small and it keeps it current)
    let _ = daemon_running;

    loop {
        // Snapshot
        let processes = snapshot::get_top_processes(top_n);

        // Load history for the period (last 12 hours = "today")
        let since = storage::now_unix() - 12 * 3600;
        let history = if daemon_running {
            let all = storage::read_all().unwrap_or_default();
            storage::aggregate_by_pid(&all, since)
        } else {
            Vec::new()
        };

        // Clear screen and print
        print!("\x1b[2J\x1b[H");
        format::print_snapshot(&processes, &history, daemon_running);

        // Footer with hint
        let _ = writeln!(
            io::stdout(),
            " {}refreshing every {interval_secs}s  ·  Ctrl+C to quit{}",
            crate::format::ansi::DIM,
            crate::format::ansi::RESET,
        );

        std::thread::sleep(interval);
    }
}
