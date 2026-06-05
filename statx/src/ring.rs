/// Fixed-size circular buffer for time-series samples.
///
/// Why a ring buffer?
///   - O(1) push — no allocation after init
///   - O(1) access to latest N samples
///   - No unbounded memory growth — statx daemon can run forever
///   - Cache-friendly — contiguous memory, sequential access
///
/// Agents query "last 60 seconds of CPU" → last 60 samples at 1s interval.
/// The ring buffer makes this a simple slice read, no sorting, no filtering.

use serde::{Deserialize, Serialize};

pub struct RingBuffer<T> {
    buf:   Vec<T>,
    cap:   usize,
    head:  usize,   // next write position
    count: usize,   // number of valid entries (≤ cap)
}

impl<T: Clone + Default> RingBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        RingBuffer {
            buf:   vec![T::default(); capacity],
            cap:   capacity,
            head:  0,
            count: 0,
        }
    }

    pub fn push(&mut self, item: T) {
        self.buf[self.head] = item;
        self.head = (self.head + 1) % self.cap;
        if self.count < self.cap {
            self.count += 1;
        }
    }

    /// Return the last `n` entries in chronological order (oldest first).
    pub fn last(&self, n: usize) -> Vec<&T> {
        let n = n.min(self.count);
        if n == 0 { return vec![]; }

        let mut result = Vec::with_capacity(n);
        // Start position: go back `n` slots from head
        let start = (self.head + self.cap - n) % self.cap;

        for i in 0..n {
            result.push(&self.buf[(start + i) % self.cap]);
        }
        result
    }

    pub fn len(&self) -> usize { self.count }
    pub fn is_empty(&self) -> bool { self.count == 0 }
}

/// A single point-in-time system snapshot
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Sample {
    pub ts:              u64,    // unix epoch seconds

    // CPU (0.0–100.0 per core, plus aggregate)
    pub cpu_total:       f32,
    pub cpu_user:        f32,
    pub cpu_system:      f32,
    pub cpu_iowait:      f32,
    pub cpu_cores:       Vec<f32>,

    // Memory (bytes)
    pub mem_total:       u64,
    pub mem_used:        u64,
    pub mem_free:        u64,
    pub mem_available:   u64,
    pub mem_cached:      u64,
    pub swap_total:      u64,
    pub swap_used:       u64,

    // Disk I/O (bytes/sec since last sample, per device)
    pub disk_read_bps:   u64,
    pub disk_write_bps:  u64,

    // Network I/O (bytes/sec since last sample, aggregate)
    pub net_rx_bps:      u64,
    pub net_tx_bps:      u64,

    // Load average
    pub load_1m:         f64,
    pub load_5m:         f64,
    pub load_15m:        f64,

    // Process counts
    pub procs_running:   u32,
    pub procs_total:     u32,
}
