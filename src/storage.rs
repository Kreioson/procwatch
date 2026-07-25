use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Data types ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SampleRow {
    pub ts: i64,
    pub pid: u32,
    pub name: String,
    pub cpu_pct: f32,
    pub mem_mb: f64,
    pub cpu_ms: u64,
    pub disk_r: u64,
    pub disk_w: u64,
}

#[derive(Debug, Clone)]
pub struct AggRow {
    pub name: String,
    pub pid: u32,
    pub cpu_secs: f64,
    pub avg_mem_mb: f64,
    pub max_mem_mb: f64,
    pub disk_r_mb: f64,
    pub disk_w_mb: f64,
    pub samples: usize,
    pub first_ts: i64,
    pub last_ts: i64,
}

#[derive(Debug)]
pub struct DbStats {
    pub sample_count: usize,
    pub db_size_bytes: u64,
    pub oldest_ts: Option<i64>,
    pub newest_ts: Option<i64>,
}

// ── Paths ───────────────────────────────────────────────────────────

pub fn get_data_dir() -> PathBuf {
    let home =
        std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".procwatch")
}

fn csv_path() -> PathBuf {
    get_data_dir().join("history.csv")
}

fn pid_path() -> PathBuf {
    get_data_dir().join("daemon.pid")
}

pub fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

// ── PID file management ─────────────────────────────────────────────

pub fn write_pid(pid: u32) -> io::Result<()> {
    let dir = get_data_dir();
    fs::create_dir_all(&dir)?;
    fs::write(pid_path(), pid.to_string())
}

pub fn read_pid() -> Option<u32> {
    fs::read_to_string(pid_path()).ok()?.trim().parse().ok()
}

pub fn remove_pid() {
    let _ = fs::remove_file(pid_path());
}

// ── CSV storage ─────────────────────────────────────────────────────

pub fn append_sample(row: &SampleRow) -> io::Result<()> {
    let dir = get_data_dir();
    fs::create_dir_all(&dir)?;

    let path = csv_path();
    let exists = path.exists();

    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    let mut w = BufWriter::new(file);

    if !exists {
        writeln!(w, "ts,pid,name,cpu_pct,mem_mb,cpu_ms,disk_r,disk_w")?;
    }

    writeln!(
        w,
        "{},{},{},{},{},{},{},{}",
        row.ts,
        row.pid,
        csv_escape(&row.name),
        row.cpu_pct,
        format_mem(row.mem_mb),
        row.cpu_ms,
        row.disk_r,
        row.disk_w,
    )?;

    w.flush()?;
    Ok(())
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn read_all() -> io::Result<Vec<SampleRow>> {
    let path = csv_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(&path)?;
    let reader = BufReader::new(file);
    let mut rows = Vec::new();
    let mut lines = reader.lines();

    // skip header
    lines.next();

    for line in lines {
        let line = line?;
        if !line.is_empty() {
            if let Ok(r) = parse_csv(&line) {
                rows.push(r);
            }
        }
    }

    Ok(rows)
}

fn parse_csv(line: &str) -> Result<SampleRow, ()> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars();

    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                fields.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    fields.push(cur);

    if fields.len() < 8 {
        return Err(());
    }

    Ok(SampleRow {
        ts: fields[0].parse().map_err(|_| ())?,
        pid: fields[1].parse().map_err(|_| ())?,
        name: fields[2].clone(),
        cpu_pct: fields[3].parse().map_err(|_| ())?,
        mem_mb: fields[4].parse().map_err(|_| ())?,
        cpu_ms: fields[5].parse().map_err(|_| ())?,
        disk_r: fields[6].parse().map_err(|_| ())?,
        disk_w: fields[7].parse().map_err(|_| ())?,
    })
}

// Write mem_mb with enough precision — CSV uses 2 decimal places stored
fn format_mem(v: f64) -> String {
    format!("{:.2}", v)
}

// ── Aggregation ─────────────────────────────────────────────────────

pub fn aggregate_by_pid(rows: &[SampleRow], since: i64) -> Vec<AggRow> {
    #[derive(Default)]
    struct Acc {
        cpu_ms_min: u64,
        cpu_ms_max: u64,
        mem_sum: f64,
        mem_max: f64,
        disk_r_min: u64,
        disk_r_max: u64,
        disk_w_min: u64,
        disk_w_max: u64,
        count: usize,
        first_ts: i64,
        last_ts: i64,
        has_data: bool,
    }

    let mut map: HashMap<(u32, String), Acc> = HashMap::new();

    for row in rows.iter().filter(|r| r.ts >= since) {
        let e = map.entry((row.pid, row.name.clone())).or_default();
        if !e.has_data {
            e.cpu_ms_min = row.cpu_ms;
            e.cpu_ms_max = row.cpu_ms;
            e.disk_r_min = row.disk_r;
            e.disk_r_max = row.disk_r;
            e.disk_w_min = row.disk_w;
            e.disk_w_max = row.disk_w;
            e.first_ts = row.ts;
            e.last_ts = row.ts;
            e.has_data = true;
        } else {
            e.cpu_ms_min = e.cpu_ms_min.min(row.cpu_ms);
            e.cpu_ms_max = e.cpu_ms_max.max(row.cpu_ms);
            e.disk_r_min = e.disk_r_min.min(row.disk_r);
            e.disk_r_max = e.disk_r_max.max(row.disk_r);
            e.disk_w_min = e.disk_w_min.min(row.disk_w);
            e.disk_w_max = e.disk_w_max.max(row.disk_w);
            e.first_ts = e.first_ts.min(row.ts);
            e.last_ts = e.last_ts.max(row.ts);
        }
        e.mem_sum += row.mem_mb;
        e.mem_max = e.mem_max.max(row.mem_mb);
        e.count += 1;
    }

    map.into_iter()
        .map(|((pid, name), a)| AggRow {
            name,
            pid,
            cpu_secs: (a.cpu_ms_max.saturating_sub(a.cpu_ms_min)) as f64 / 1000.0,
            avg_mem_mb: if a.count > 0 { a.mem_sum / a.count as f64 } else { 0.0 },
            max_mem_mb: a.mem_max,
            disk_r_mb: (a.disk_r_max.saturating_sub(a.disk_r_min)) as f64 / 1_048_576.0,
            disk_w_mb: (a.disk_w_max.saturating_sub(a.disk_w_min)) as f64 / 1_048_576.0,
            samples: a.count,
            first_ts: a.first_ts,
            last_ts: a.last_ts,
        })
        .collect()
}

// ── Pruning & stats ─────────────────────────────────────────────────

pub fn prune_old(before: i64) -> io::Result<usize> {
    let rows = read_all()?;
    let keep: Vec<&SampleRow> = rows.iter().filter(|r| r.ts >= before).collect();

    if keep.len() == rows.len() {
        return Ok(0);
    }

    // Rewrite the file with kept rows
    let path = csv_path();
    let file = File::create(&path)?;
    let mut w = BufWriter::new(file);
    writeln!(w, "ts,pid,name,cpu_pct,mem_mb,cpu_ms,disk_r,disk_w")?;
    for r in &keep {
        writeln!(
            w,
            "{},{},{},{},{},{},{},{}",
            r.ts, r.pid, csv_escape(&r.name), r.cpu_pct, format_mem(r.mem_mb), r.cpu_ms, r.disk_r, r.disk_w,
        )?;
    }
    w.flush()?;

    Ok(rows.len() - keep.len())
}

pub fn get_stats() -> DbStats {
    let path = csv_path();
    let size = path.metadata().map(|m| m.len()).unwrap_or(0);

    let rows = read_all().unwrap_or_default();
    let count = rows.len();

    let oldest = rows.iter().map(|r| r.ts).min();
    let newest = rows.iter().map(|r| r.ts).max();

    DbStats { sample_count: count, db_size_bytes: size, oldest_ts: oldest, newest_ts: newest }
}

// ── Daemon helper ───────────────────────────────────────────────────

pub fn daemon_is_running() -> bool {
    let pid = match read_pid() {
        Some(p) => p,
        None => return false,
    };
    // Check via sysinfo using a minimal system refresh
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(
        sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]),
        false,
    );
    sys.process(sysinfo::Pid::from_u32(pid)).is_some()
}
