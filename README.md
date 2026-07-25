# procwatch

**Ultra-minimal process historian** — track what processes eat your CPU, memory, and disk over time.

```
547 KB     ·     1 dependency (sysinfo)     ·     Rust     ·     Linux / Windows
```

```text
 PID  PROCESS         CPU%   MEM   CPU Δ        DISK Δ
 ─────────────────────────────────────────────────────────
 1849 kitty             49% ███░░░   162M       22s  16KR │ 0KW
  938 hermes             2% ░░░░░░   288M        2s   0KR │ 0KW
```

## One-shot (no setup)

```bash
procwatch
```

Shows current top processes with live CPU%, memory, and a warning if the daemon isn't running.

## Daemon mode (history tracking)

```bash
# Start the background sampler (30s interval)
procwatch daemon

# View live data + cumulative history
procwatch

# Query historical resource usage
procwatch history --since 1h --sort cpu
procwatch history --since 24h --name firefox

# Live-refresh mode (like top with history columns)
procwatch watch

# Stop the daemon
procwatch stop
```

## Commands

| Command | Description |
|---------|-------------|
| `procwatch` / `ps` | Live top processes + today's cumulative history |
| `daemon` | Start background sampler (auto-daemonizes on Unix) |
| `watch` | Live-refresh mode, updates every 2s |
| `history` | Query historical data by period |
| `status` | Check daemon status and database stats |
| `stop` | Stop the background daemon |
| `help` | Colored, example-rich help |

### History options

| Flag | Description | Default |
|------|-------------|---------|
| `--since 1h` | Time window: 1h, 6h, 24h, 7d | 1h |
| `--pid 1234` | Filter by PID | all |
| `--name firefox` | Filter by process name (substring) | all |
| `--top 10` | Limit results | 15 |
| `--sort cpu` | Sort by: cpu, mem, disk | cpu |

## How it works

The daemon samples the top 15 processes every 30 seconds and appends to a plain CSV file at `~/.procwatch/history.csv`. Queries read the CSV into memory and aggregate by PID — no database server, no binary format, you can `grep` your history directly.

```csv
ts,pid,name,cpu_pct,mem_mb,cpu_ms,disk_r,disk_w
1711500000,4523,firefox.exe,12.4,1248.2,2048000,1073741824,524288000
```

## Install

### Linux

```bash
sudo cp procwatch /usr/local/bin/
```

### Windows — step by step

**1.** Download `procwatch-v0.1.0-x86_64-windows.exe` from the [releases page](https://github.com/Kreioson/procwatch/releases).

**2.** Create a folder for it (no admin needed):

```powershell
mkdir C:\Tools\procwatch
```

Move or copy the exe there and rename it to `procwatch.exe`.

**3.** Add it to PATH so you can type `procwatch` from any terminal:

```powershell
# Run this in PowerShell (NOT as admin — it's per-user)
[Environment]::SetEnvironmentVariable(
    "PATH",
    [Environment]::GetEnvironmentVariable("PATH", "User") + ";C:\Tools\procwatch",
    "User"
)
```

Restart your terminal. Now `procwatch help` works everywhere.

**4.** Start the daemon (one time per boot, or set it to auto-start):

```powershell
# Manual start — stays on until reboot or you stop it
procwatch daemon
```

**5.** Use it:

```powershell
procwatch                 # live processes + history
procwatch watch           # live-refresh mode
procwatch watch --sort mem  # sorted by memory
procwatch history --since 1h --sort cpu  # top offenders
```

**6.** Stop the daemon when you want:

```powershell
taskkill /F /IM procwatch.exe
```

#### Auto-start on boot (Task Scheduler)

Run this **once as Administrator** — the daemon starts 30s after you log in, survives reboots, and auto-restarts if it crashes:

```powershell
schtasks /Create /SC ONLOGON /TN "procwatch-daemon" /TR "C:\Tools\procwatch\procwatch.exe daemon" /DELAY 0000:30 /RL HIGHEST /IT /F
```

To remove it later:

```powershell
schtasks /Delete /TN "procwatch-daemon" /F
```

### Build from source

```bash
cargo build --release
# Binary: target/release/procwatch (Linux)
# Binary: target/release/procwatch.exe (Windows)
```

## Size

| Platform | Binary size | Runtime RAM | Storage (7 days @ 30s) |
|----------|------------|-------------|----------------------|
| Linux    | ~550 KB    | ~4-6 MB     | ~18 MB CSV |
| Windows  | ~475 KB    | ~4-6 MB     | ~18 MB CSV |

## Why

No Electron. No Python. No SQLite. No GUI. No tray icon. One flat file, one syscall library, one binary.
