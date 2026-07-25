mod daemon;
mod format;
mod snapshot;
mod storage;
mod watch;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bin_name = args.first().map(|s| s.as_str()).unwrap_or("procwatch");

    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("ps");
    let rest = if args.len() > 2 { &args[2..] } else { &[] };

    match cmd {
        "daemon" => {
            let interval = find_arg(rest, "--interval").map_or(30, |v| parse_duration(v).unwrap_or(30) as u64);
            daemon::run_daemon(Some(interval));
        }

        "stop" => {
            daemon::stop_daemon();
        }

        "status" => {
            let running = storage::daemon_is_running();
            let stats = storage::get_stats();
            format::print_status(&stats, running);
        }

        "watch" => {
            let interval = find_arg(rest, "--interval").map_or(2, |v| parse_duration(v).unwrap_or(2) as u64);
            let sort = find_arg(rest, "--sort").unwrap_or("cpu");
            watch::run_watch(interval, sort);
        }

        "history" => {
            let since = find_arg(rest, "--since").unwrap_or("1h");
            let since_secs = storage::now_unix() - parse_duration(since).unwrap_or(3600);

            let filter_pid = find_arg(rest, "--pid")
                .and_then(|s| s.parse::<u32>().ok());

            let filter_name = find_arg(rest, "--name");

            let top_n = find_arg(rest, "--top")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(15);

            let sort = find_arg(rest, "--sort").unwrap_or("cpu");

            let all = match storage::read_all() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("procwatch: failed to read history: {e}");
                    std::process::exit(1);
                }
            };

            let mut history = storage::aggregate_by_pid(&all, since_secs);

            if let Some(pid) = filter_pid {
                history.retain(|h| h.pid == pid);
            }

            if let Some(name) = filter_name {
                history.retain(|h| h.name.to_lowercase().contains(&name.to_lowercase()));
            }

            match sort {
                "mem" | "memory" => history.sort_by(|a, b| {
                    b.avg_mem_mb
                        .partial_cmp(&a.avg_mem_mb)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }),
                "disk" => history.sort_by(|a, b| {
                    let a_total = a.disk_r_mb + a.disk_w_mb;
                    let b_total = b.disk_r_mb + b.disk_w_mb;
                    b_total
                        .partial_cmp(&a_total)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }),
                _ => history.sort_by(|a, b| {
                    b.cpu_secs
                        .partial_cmp(&a.cpu_secs)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }),
            }

            history.truncate(top_n);
            format::print_history(&history, since);
        }

        "ps" | "" if cmd == "ps" || args.len() == 1 => {
            let running = storage::daemon_is_running();
            let processes = snapshot::get_top_processes(15);

            let history = if running {
                let since = storage::now_unix() - 12 * 3600;
                let all = storage::read_all().unwrap_or_default();
                storage::aggregate_by_pid(&all, since)
            } else {
                Vec::new()
            };

            format::print_snapshot(&processes, &history, running);
        }

        "help" | "--help" => {
            format::print_help(bin_name);
        }

        "--version" => {
            println!("procwatch {}", env!("CARGO_PKG_VERSION"));
        }

        other => {
            eprintln!("procwatch: unknown command '{other}'");
            eprintln!("Run `{bin_name} help` for usage.");
            std::process::exit(1);
        }
    }
}

/// Find the value of a named flag in args (e.g. --since, --name etc.)
fn find_arg<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
}

/// Parse a simple duration string like "30s", "5m", "2h", "7d" into seconds.
fn parse_duration(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num_str, unit) = s.split_at(s.len() - 1);
    let num: i64 = num_str.parse().ok()?;
    match unit {
        "s" => Some(num),
        "m" => Some(num * 60),
        "h" => Some(num * 3600),
        "d" => Some(num * 86400),
        _ => None,
    }
}
