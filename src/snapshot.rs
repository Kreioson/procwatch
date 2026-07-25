use std::time::Duration;

/// One process sample from the live system
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_pct: f32,
    pub mem_mb: f64,
    pub cpu_ms: u64,
    pub disk_r: u64,
    pub disk_w: u64,
}

/// Get the top-N processes sorted by CPU % descending.
/// Returns up to `count` entries.
pub fn get_top_processes(count: usize) -> Vec<ProcessInfo> {
    let mut sys = sysinfo::System::new();

    // First pass: load process list with CPU (establishes baseline)
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    sys.refresh_memory();

    // Wait so we get a meaningful CPU delta
    std::thread::sleep(Duration::from_millis(500));

    // Second pass: refreshes processes AND computes CPU delta
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let mut processes: Vec<ProcessInfo> = sys
        .processes()
        .iter()
        .map(|(_, p)| {
            let mem_bytes = p.memory();
            let disk = p.disk_usage();
            let cpu = p.cpu_usage();
            let cpu_time = p.accumulated_cpu_time();

            ProcessInfo {
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
        b.cpu_pct
            .partial_cmp(&a.cpu_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    processes.truncate(count);
    processes
}
