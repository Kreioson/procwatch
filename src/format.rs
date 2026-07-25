use std::io::{self, Write};

use crate::snapshot::ProcessInfo;
use crate::storage::{AggRow, DbStats};

// ── ANSI helpers ────────────────────────────────────────────────────

pub mod ansi {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";

    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const CYAN: &str = "\x1b[36m";
    pub const GRAY: &str = "\x1b[90m";

    pub fn cpu_color(pct: f32) -> &'static str {
        if pct > 50.0 { RED } else if pct > 20.0 { YELLOW } else { GREEN }
    }

    pub fn mem_color(mb: f64) -> &'static str {
        if mb > 2000.0 { RED } else if mb > 500.0 { YELLOW } else { GREEN }
    }
}

// ── Terminal width ──────────────────────────────────────────────────

pub fn terminal_width() -> usize {
    // Try COLUMNS env var first
    if let Ok(w) = std::env::var("COLUMNS") {
        if let Ok(w) = w.trim().parse::<usize>() {
            if w >= 40 {
                return w;
            }
        }
    }
    // Try `stty size` on Unix
    #[cfg(unix)]
    {
        if let Ok(out) = std::process::Command::new("stty")
            .arg("size")
            .arg("-F")
            .arg("/dev/tty")
            .output()
        {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(w) = s.split_whitespace().nth(1).and_then(|n| n.parse::<usize>().ok()) {
                if w >= 40 {
                    return w;
                }
            }
        }
    }
    80
}

// ── Human-readable formatting ───────────────────────────────────────

fn fmt_mem(mb: f64) -> String {
    if mb >= 1024.0 {
        format!("{:.1}G", mb / 1024.0)
    } else {
        format!("{:.0}M", mb)
    }
}

fn fmt_disk(mb: f64) -> String {
    if mb >= 1024.0 {
        format!("{:.1}G", mb / 1024.0)
    } else if mb >= 1.0 {
        format!("{:.0}M", mb)
    } else {
        format!("{:.0}K", mb * 1024.0)
    }
}

fn fmt_cpu_secs(secs: f64) -> String {
    if secs >= 3600.0 {
        let h = (secs / 3600.0) as u64;
        let m = ((secs % 3600.0) / 60.0) as u64;
        format!("{}h {}m", h, m)
    } else if secs >= 60.0 {
        let m = (secs / 60.0) as u64;
        let s = (secs % 60.0) as u64;
        format!("{}m {:02}s", m, s)
    } else {
        format!("{:.0}s", secs)
    }
}

fn fmt_bar(pct: f32, width: usize) -> String {
    if width < 2 {
        return String::new();
    }
    let filled = ((pct / 100.0) * width as f32).round() as usize;
    let filled = filled.min(width);
    let empty = width.saturating_sub(filled);
    let color = ansi::cpu_color(pct);
    format!(
        "{}{}{}{}{}",
        color,
        "█".repeat(filled),
        ansi::DIM,
        "░".repeat(empty),
        ansi::RESET,
    )
}

fn fmt_timestamp(ts: i64) -> String {
    // Simple UTC display without chrono
    let s = ts as u64;
    let _days = s / 86400;
    let time_secs = s % 86400;
    let h = time_secs / 3600;
    let m = (time_secs % 3600) / 60;
    let sec = time_secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, sec)
}

fn fmt_date(ts: i64) -> String {
    let s = ts as u64;
    let _days = s / 86400;
    // Approximate from unix epoch
    let time_secs = s % 86400;
    let h = time_secs / 3600;
    let m = (time_secs % 3600) / 60;
    format!("{:02}:{:02}", h, m)
}

fn fmt_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1}G", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.0}M", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.0}K", bytes as f64 / 1024.0)
    } else {
        format!("{}B", bytes)
    }
}

// ── Table drawing ───────────────────────────────────────────────────

// (hline intentionally omitted — not needed)

fn truncated(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max >= 3 {
        format!("{}…", &s[..max.saturating_sub(1)])
    } else {
        s.chars().take(max).collect()
    }
}

// ── PS / default view ───────────────────────────────────────────────

pub fn print_snapshot(processes: &[ProcessInfo], history: &[AggRow], daemon_running: bool) {
    let width = terminal_width();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Build history lookup: pid -> AggRow
    let mut hist_by_pid: std::collections::HashMap<u32, &AggRow> = std::collections::HashMap::new();
    for h in history {
        hist_by_pid.insert(h.pid, h);
    }

    // Also match by name as fallback
    let mut hist_by_name: std::collections::HashMap<&str, &AggRow> = std::collections::HashMap::new();
    for h in history {
        hist_by_name.insert(&h.name, h);
    }

    // Determine columns based on width
    let has_history = daemon_running && !history.is_empty();
    let wide = width >= 90;
    let medium = width >= 60;

    // Header
    let status_dot = if daemon_running {
        format!("{}●{}", ansi::GREEN, ansi::RESET)
    } else {
        format!("{}○{}", ansi::RED, ansi::RESET)
    };

    let hist_label = if has_history {
        let now = crate::storage::now_unix();
        // Show "today" as last 12h for simplicity
        let _start_of_period = now.saturating_sub(12 * 3600);
        let samples: usize = history.iter().map(|h| h.samples).sum();
        format!(" │ {} samples │ last 12h", samples)
    } else {
        String::new()
    };

    let _ = writeln!(out);
    let _ = writeln!(
        out,
        " {}{}procwatch{} {} {}│ daemon {}{}",
        ansi::BOLD,
        ansi::CYAN,
        ansi::RESET,
        fmt_timestamp(crate::storage::now_unix()),
        status_dot,
        hist_label,
        ansi::RESET,
    );

    let _ = writeln!(out);

    if !daemon_running {
        let _ = writeln!(
            out,
            " {}⚠ daemon not running — run `procwatch daemon` for history tracking{}",
            ansi::YELLOW,
            ansi::RESET,
        );
        let _ = writeln!(out);
    }

    // Table header
    let mut header = String::from(" PID  ");
    header.push_str(&pad_right("PROCESS", if wide { 18 } else if medium { 14 } else { 10 }));
    header.push_str("  CPU%   MEM");
    if wide {
        header.push_str("   CPU Δ        DISK Δ");
    } else if medium {
        header.push_str("   CPU Δ");
    }

    let _ = writeln!(out, " {}{}{}", ansi::DIM, ansi::BOLD, header);

    // Separator
    let mut sep = String::from("──────");
    let name_w = if wide { 18 } else if medium { 14 } else { 10 };
    sep.push_str(&"─".repeat(name_w + 2));
    sep.push_str("─────────────");
    if wide {
        sep.push_str("──────────────────");
    } else if medium {
        sep.push_str("──────────");
    }
    let _ = writeln!(out, " {}{}{}", ansi::DIM, sep, ansi::RESET);

    // Rows
    for (i, p) in processes.iter().enumerate() {
        let hist = hist_by_pid.get(&p.pid).copied()
            .or_else(|| hist_by_name.get(p.name.as_str()).copied());

        let cpu_color = ansi::cpu_color(p.cpu_pct);
        let mem_color = ansi::mem_color(p.mem_mb);
        let dim = if i % 2 == 0 { ansi::DIM } else { "" };

        let name = truncated(&p.name, name_w);

        let _ = write!(out, " {}{:<5}{}  {}{:<width$}{}  ",
            dim, p.pid, ansi::RESET,
            ansi::BOLD, name, ansi::RESET,
            width = name_w,
        );

        // CPU% with bar
        if wide {
            let bar = fmt_bar(p.cpu_pct, 6);
            let _ = write!(out, "{}{:>4.0}% {}  {}", cpu_color, p.cpu_pct, bar, ansi::RESET);
        } else {
            let _ = write!(out, "{}{:>5.1}%{}  ", cpu_color, p.cpu_pct, ansi::RESET);
        }

        // MEM
        let _ = write!(out, "{}{:>5}{}  ", mem_color, fmt_mem(p.mem_mb), ansi::RESET);

        // CPU Δ (history)
        if medium {
            if let Some(h) = hist {
                let _ = write!(out, "{:>8}  ", fmt_cpu_secs(h.cpu_secs));
            } else if has_history {
                let _ = write!(out, "{}  —    {}  ", ansi::GRAY, ansi::RESET);
            } else {
                let _ = write!(out, "{}  —    {}  ", ansi::GRAY, ansi::RESET);
            }
        }

        // DISK Δ
        if wide {
            if let Some(h) = hist {
                let _ = write!(out, "{}R {}│{} {}W",
                    fmt_disk(h.disk_r_mb),
                    ansi::DIM,
                    ansi::RESET,
                    fmt_disk(h.disk_w_mb),
                );
            } else {
                let _ = write!(out, "{}—{}", ansi::GRAY, ansi::RESET);
            }
        }

        let _ = writeln!(out);
    }
    let _ = writeln!(out);
}

// ── History view ────────────────────────────────────────────────────

pub fn print_history(history: &[AggRow], since_label: &str) {
    let width = terminal_width();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let _ = writeln!(out);
    let _ = writeln!(
        out,
        " {}▸ procwatch history — last {}{}",
        ansi::CYAN,
        since_label,
        ansi::RESET,
    );
    let _ = writeln!(out);

    if history.is_empty() {
        let _ = writeln!(out, " {}no data in this period{}", ansi::GRAY, ansi::RESET);
        let _ = writeln!(out);
        return;
    }

    let wide = width >= 100;

    // Header
    let _ = writeln!(
        out,
        " {}{}  #  {:18} {:>10} {:>8} {:>8} {:>8}│{:<8}  {}samples{}",
        ansi::BOLD,
        if wide { "" } else { "" },
        "PROCESS",
        "CPU TIME",
        "AVG MEM",
        "PEAK MEM",
        "DISK R",
        "W",
        ansi::GRAY,
        ansi::RESET,
    );

    let _ = writeln!(
        out,
        " {}──────────────────────────────────────────────────────────────{}",
        ansi::DIM, ansi::RESET
    );

    for (i, h) in history.iter().enumerate() {
        let rank = i + 1;
        let name = truncated(&h.name, if wide { 18 } else { 14 });

        let cpu_color = if h.cpu_secs > 3600.0 { ansi::RED } else if h.cpu_secs > 600.0 { ansi::YELLOW } else { ansi::GREEN };
        let dim = if i % 2 == 0 { ansi::DIM } else { "" };

        let _ = writeln!(
            out,
            " {}{:<3} {}{:<18}{} {:>10} {:>8} {:>8} {:>8}│{:<8}  {:<5}",
            dim,
            format!("#{}", rank),
            ansi::BOLD,
            name,
            ansi::RESET,
            format!("{}{}{}", cpu_color, fmt_cpu_secs(h.cpu_secs), ansi::RESET),
            fmt_mem(h.avg_mem_mb),
            fmt_mem(h.max_mem_mb),
            fmt_disk(h.disk_r_mb),
            fmt_disk(h.disk_w_mb),
            h.samples,
        );
    }
    let _ = writeln!(out);
}

// ── Status view ─────────────────────────────────────────────────────

pub fn print_status(stats: &DbStats, daemon_running: bool) {
    println!();
    if daemon_running {
        let pid = crate::storage::read_pid().unwrap_or(0);
        println!(" {}●{}  procwatch daemon {}running{} (PID {})",
            ansi::GREEN, ansi::RESET,
            ansi::BOLD, ansi::RESET, pid,
        );
    } else {
        println!(" {}○{}  procwatch daemon {}not running{}",
            ansi::RED, ansi::RESET,
            ansi::BOLD, ansi::RESET,
        );
    }
    println!();
    println!("  data dir    {}", crate::storage::get_data_dir().display());
    println!("  DB size     {}", fmt_size(stats.db_size_bytes));
    println!("  samples     {}", stats.sample_count);
    if let Some(old) = stats.oldest_ts {
        println!("  oldest      {}", fmt_timestamp(old));
    }
    if let Some(new) = stats.newest_ts {
        println!("  newest      {}", fmt_timestamp(new));
    }
    println!();
}

// ── Help text ───────────────────────────────────────────────────────

pub fn print_help(bin_name: &str) {
    let b = |s: &str| format!("{}{}{}{}", ansi::BOLD, ansi::CYAN, s, ansi::RESET);
    let cmd = |s: &str| format!("{}{}{}", ansi::BOLD, s, ansi::RESET);
    let desc = |s: &str| format!("{}", s);
    let dim = |s: &str| format!("{}{}{}", ansi::DIM, s, ansi::RESET);

    println!("{}", b("procwatch — minimal process historian"));
    println!("{}", dim("Track what processes consume your CPU, memory, and disk over time."));
    println!();

    println!("{}", b("USAGE"));
    println!("  {} [{}]", cmd(bin_name), dim("command"));
    println!();

    println!("{}", b("COMMANDS"));
    println!("  {}  {}", cmd("ps"),      desc("Show live top processes + today's history (default)"));
    println!("  {}  {}", cmd("watch"),   desc("Live-refresh mode, updates every 2s"));
    println!("  {}  {}", cmd("daemon"),  desc("Start background sampler (30s interval)"));
    println!("  {}  {}", cmd("stop"),    desc("Stop the background daemon"));
    println!("  {}  {}", cmd("status"),  desc("Check daemon status and DB stats"));
    println!("  {}  {}", cmd("history"), desc("Query historical data by period"));
    println!("  {}  {}", cmd("help"),    desc("Show this help"));
    println!();

    println!("{}", b("HISTORY OPTIONS"));
    println!("  {} <dur>   Time window: 1h, 6h, 24h, 7d  {}", dim("--since"), dim("(default: 1h)"));
    println!("  {} <pid>   Filter by PID              {}", dim("--pid"),   dim("(optional)"));
    println!("  {} <name>  Filter by process name      {}", dim("--name"),  dim("(optional, substring)"));
    println!("  {} <n>     Show top N results          {}", dim("--top"),   dim("(default: 15)"));
    println!("  {} <col>   Sort column: cpu, mem, disk {}", dim("--sort"),  dim("(default: cpu)"));
    println!();

    println!("{}", b("EXAMPLES"));
    println!("  {}                          # Live view with history", cmd(bin_name));
    println!("  {}                          # Same as above", cmd(&format!("{} ps", bin_name)));
    println!("  {}                          # Start background tracker", cmd(&format!("{} daemon", bin_name)));
    println!("  {}              # Top processes in last 6h", cmd(&format!("{} history --since 6h", bin_name)));
    println!("  {}     # Firefox usage today", cmd(&format!("{} history --since 24h --name firefox", bin_name)));
    println!("  {}         # Live-refresh mode", cmd(&format!("{} watch", bin_name)));
    println!("  {}                          # Daemon status", cmd(&format!("{} status", bin_name)));
    println!();

    println!("{}", b("DATA"));
    println!("  CSV file: {}", crate::storage::get_data_dir().join("history.csv").display());
    println!("  PID file: {}", crate::storage::get_data_dir().join("daemon.pid").display());
    println!();
}

fn pad_right(s: &str, width: usize) -> String {
    if s.len() >= width {
        s[..width].to_string()
    } else {
        let mut r = s.to_string();
        r.push_str(&" ".repeat(width - s.len()));
        r
    }
}
