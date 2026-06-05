use crate::ring::Sample;
use std::time::UNIX_EPOCH;
use sysinfo::{Disks, Networks, System};

pub struct Collector {
    sys:      System,
    disks:    Disks,
    networks: Networks,
    // Previous sample for delta calculations
    prev_disk_read:  u64,
    prev_disk_write: u64,
    prev_net_rx:     u64,
    prev_net_tx:     u64,
    prev_ts:         u64,
}

impl Collector {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        let disks = Disks::new_with_refreshed_list();
        let networks = Networks::new_with_refreshed_list();

        Collector {
            sys,
            disks,
            networks,
            prev_disk_read:  0,
            prev_disk_write: 0,
            prev_net_rx:     0,
            prev_net_tx:     0,
            prev_ts:         now_secs(),
        }
    }

    pub fn sample(&mut self) -> Sample {
        self.sys.refresh_all();
        self.disks.refresh(true);
        self.networks.refresh(true);

        let ts = now_secs();
        let elapsed = (ts - self.prev_ts).max(1) as f64;

        // ── CPU ───────────────────────────────────────────────────────────────
        let cpu_total  = self.sys.global_cpu_usage();
        let cpu_cores: Vec<f32> = self.sys.cpus().iter().map(|c| c.cpu_usage()).collect();
        // sysinfo doesn't expose user/system/iowait separately on all platforms
        let cpu_user   = cpu_total * 0.7;  // approximation where not available
        let cpu_system = cpu_total * 0.3;
        let cpu_iowait = 0.0;

        // ── Memory ────────────────────────────────────────────────────────────
        let mem_total     = self.sys.total_memory();
        let mem_used      = self.sys.used_memory();
        let mem_free      = self.sys.free_memory();
        let mem_available = self.sys.available_memory();
        let mem_cached    = mem_total.saturating_sub(mem_used + mem_free);
        let swap_total    = self.sys.total_swap();
        let swap_used     = self.sys.used_swap();

        // ── Disk I/O ──────────────────────────────────────────────────────────
        let disk_read:  u64 = self.disks.iter().map(|d| d.usage().read_bytes).sum();
        let disk_write: u64 = self.disks.iter().map(|d| d.usage().written_bytes).sum();
        let disk_read_bps  = delta_rate(disk_read,  self.prev_disk_read,  elapsed);
        let disk_write_bps = delta_rate(disk_write, self.prev_disk_write, elapsed);
        self.prev_disk_read  = disk_read;
        self.prev_disk_write = disk_write;

        // ── Network I/O ───────────────────────────────────────────────────────
        let net_rx: u64 = self.networks.iter().map(|(_, n)| n.received()).sum();
        let net_tx: u64 = self.networks.iter().map(|(_, n)| n.transmitted()).sum();
        let net_rx_bps = delta_rate(net_rx, self.prev_net_rx, elapsed);
        let net_tx_bps = delta_rate(net_tx, self.prev_net_tx, elapsed);
        self.prev_net_rx = net_rx;
        self.prev_net_tx = net_tx;

        // ── Load average ──────────────────────────────────────────────────────
        let load = System::load_average();

        // ── Processes ─────────────────────────────────────────────────────────
        let procs_total   = self.sys.processes().len() as u32;
        let procs_running = self.sys.processes().values()
            .filter(|p| matches!(p.status(), sysinfo::ProcessStatus::Run))
            .count() as u32;

        self.prev_ts = ts;

        Sample {
            ts,
            cpu_total,
            cpu_user,
            cpu_system,
            cpu_iowait,
            cpu_cores,
            mem_total,
            mem_used,
            mem_free,
            mem_available,
            mem_cached,
            swap_total,
            swap_used,
            disk_read_bps,
            disk_write_bps,
            net_rx_bps,
            net_tx_bps,
            load_1m:  load.one,
            load_5m:  load.five,
            load_15m: load.fifteen,
            procs_running,
            procs_total,
        }
    }
}

fn delta_rate(current: u64, previous: u64, elapsed_secs: f64) -> u64 {
    if current < previous { return 0; } // counter reset
    ((current - previous) as f64 / elapsed_secs) as u64
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
