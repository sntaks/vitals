//! Reads numbers from the OS on a background thread, so a slow refresh
//! (walking every process can take tens of ms on Windows) never stalls a frame.
//!
//! Flow: [`spawn`] starts a thread that owns a [`Sampler`]. Every [`REFRESH`] it
//! takes a [`Sample`], sends it down a channel and pokes the UI to repaint.
//! The UI picks samples up in `app.rs` and never touches sysinfo itself.

use crate::config::{REFRESH, TOP_N};
use eframe::egui;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Instant;
use sysinfo::{
    CpuRefreshKind, Disks, MemoryRefreshKind, Networks, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System,
    MINIMUM_CPU_UPDATE_INTERVAL,
};

/// One row of the process table: every running process that shares a name, added together.
#[derive(Clone, Debug)]
pub struct ProcRow {
    pub name: String,
    /// How many processes were merged into this row.
    pub count: usize,
    /// % of the whole machine (all cores), like Task Manager.
    pub cpu: f32,
    /// Bytes of RAM (resident set / working set), summed over the group.
    pub mem: u64,
    /// Bytes per second read + written.
    pub io: f64,
}

/// One reading of everything we show.
#[derive(Clone, Debug)]
pub struct Sample {
    /// When it was taken; drives the sparkline scroll.
    pub at: Instant,
    /// Whole-machine CPU use, 0..=100.
    pub cpu: f32,
    /// Per-core CPU use, 0..=100 each.
    pub cores: Vec<f32>,
    pub mem_used: u64,
    pub mem_total: u64,
    /// Bytes per second, summed over all processes (see docs/sampling.md for what this counts).
    pub disk_read: f64,
    pub disk_write: f64,
    /// Space on the main drive, in bytes.
    pub disk_used: u64,
    pub disk_total: u64,
    /// Bytes per second, summed over all network interfaces.
    pub net_down: f64,
    pub net_up: f64,
    /// The busiest TOP_N process groups, busiest first.
    pub procs: Vec<ProcRow>,
}

/// Owns the sysinfo handles. Lives on the sampler thread only.
struct Sampler {
    sys: System,
    networks: Networks,
    disks: Disks,
    /// When the previous refresh happened. Counters are deltas since then, so
    /// dividing by the elapsed time turns them into per-second rates.
    last: Instant,
}

impl Sampler {
    fn new() -> Self {
        // Only request for things we actually use: keeps it light
        let sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
                .with_memory(MemoryRefreshKind::nothing().with_ram()),
        );
        let mut sampler = Self {
            sys,
            networks: Networks::new_with_refreshed_list(),
            disks: Disks::new_with_refreshed_list(),
            last: Instant::now(),
        };
        // Prime the counters. CPU usage and per-process I/O are deltas between two
        // refreshes; on the very first one sysinfo reports lifetime totals instead.
        sampler.refresh_os();
        sampler.last = Instant::now();
        sampler
    }

    /// Asks the OS for fresh numbers. Expensive part: walks every process.
    fn refresh_os(&mut self) {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            // Forget processes that have exited.
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory().with_disk_usage(),
        );
        self.networks.refresh(true);
        self.disks.refresh(false);
    }

    // Read fresh numbers from OS
    fn sample(&mut self) -> Sample {
        self.refresh_os();
        let at = Instant::now();
        let secs = at.duration_since(self.last).as_secs_f64().max(0.001);
        self.last = at;

        let core_count = self.sys.cpus().len().max(1) as f32;

        // Network: bytes moved since last refresh, divided by time = speed.
        let (mut down, mut up) = (0u64, 0u64);
        for (_name, data) in &self.networks {
            down += data.received();
            up += data.transmitted();
        }

        // Disk space of the main drive ("/" on Linux, "C:\" on Windows).
        let (disk_used, disk_total) = self
            .disks
            .iter()
            .find(|d| {
                let m = d.mount_point().to_string_lossy();
                m == "/" || m.to_uppercase().starts_with("C:")
            })
            .or_else(|| self.disks.iter().next())
            .map(|d| (d.total_space().saturating_sub(d.available_space()), d.total_space()))
            .unwrap_or_default();

        // Processes: add up I/O for the whole machine, and merge processes that share a
        // name (a browser runs dozens of helpers) so each app gets a single row.
        let (mut rd, mut wr) = (0u64, 0u64);
        let mut groups: HashMap<String, ProcRow> = HashMap::new();
        for (pid, p) in self.sys.processes() {
            // PID 0 is the Windows "Idle" pseudo-process, never a real workload.
            if pid.as_u32() == 0 {
                continue;
            }
            let du = p.disk_usage();
            rd += du.read_bytes;
            wr += du.written_bytes;

            let name = p.name().to_string_lossy();
            let group = groups.entry(name.into_owned()).or_insert_with_key(|name| ProcRow {
                name: name.clone(),
                count: 0,
                cpu: 0.0,
                mem: 0,
                io: 0.0,
            });
            group.count += 1;
            // sysinfo gives % of ONE core; divide to match Task Manager.
            group.cpu += p.cpu_usage() / core_count;
            group.mem += p.memory();
            group.io += (du.read_bytes + du.written_bytes) as f64 / secs;
        }
        let procs = top_groups(groups.into_values().collect());

        Sample {
            at,
            cpu: self.sys.global_cpu_usage(),
            cores: self.sys.cpus().iter().map(|c| c.cpu_usage()).collect(),
            mem_used: self.sys.used_memory(),
            mem_total: self.sys.total_memory(),
            disk_read: rd as f64 / secs,
            disk_write: wr as f64 / secs,
            disk_used,
            disk_total,
            net_down: down as f64 / secs,
            net_up: up as f64 / secs,
            procs,
        }
    }
}

/// Keeps the busiest TOP_N groups, busiest first.
///
/// Ties (common: many groups sit at 0.0%) are broken by memory, then name. Without
/// that, the order would come from HashMap iteration, which is random per map, so
/// tied rows would swap places, and animate, on every reading.
fn top_groups(mut groups: Vec<ProcRow>) -> Vec<ProcRow> {
    groups.sort_by(|a, b| {
        b.cpu
            .total_cmp(&a.cpu)
            .then(b.mem.cmp(&a.mem))
            .then_with(|| a.name.cmp(&b.name))
    });
    groups.truncate(TOP_N);
    groups
}

/// Starts sampling every [`REFRESH`] on its own thread. The thread wakes the UI
/// after each reading and exits on its own once the receiver is dropped.
pub fn spawn(ctx: egui::Context) -> Receiver<Sample> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("vitals-sampler".into())
        .spawn(move || {
            let mut sampler = Sampler::new();
            // Shortest wait sysinfo needs for meaningful CPU numbers, so the first reading shows up fast.
            thread::sleep(MINIMUM_CPU_UPDATE_INTERVAL);
            let mut next = Instant::now();
            loop {
                // Sending fails only when the app has closed and dropped the receiver: stop.
                if tx.send(sampler.sample()).is_err() {
                    break;
                }
                // The UI may be asleep (nothing animating); wake it to show the new reading.
                ctx.request_repaint();
                // Schedule against a fixed clock so slow refreshes don't make the interval drift.
                next += REFRESH;
                let now = Instant::now();
                if next > now {
                    thread::sleep(next - now);
                } else {
                    // We fell behind (e.g. the machine was asleep): don't try to catch up in a burst.
                    next = now;
                }
            }
        })
        .expect("failed to start sampler thread");
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(name: &str, cpu: f32, mem: u64) -> ProcRow {
        ProcRow { name: name.to_owned(), count: 1, cpu, mem, io: 0.0 }
    }

    #[test]
    fn busiest_first_and_truncated() {
        let groups = (0..TOP_N + 3).map(|i| group(&format!("p{i}"), i as f32, 0)).collect();
        let top = top_groups(groups);
        assert_eq!(top.len(), TOP_N);
        assert_eq!(top[0].name, format!("p{}", TOP_N + 2));
    }

    #[test]
    fn ties_have_a_stable_order() {
        let a = vec![group("b", 0.0, 10), group("a", 0.0, 10), group("c", 0.0, 99)];
        let b = vec![group("a", 0.0, 10), group("c", 0.0, 99), group("b", 0.0, 10)];
        let names = |v: Vec<ProcRow>| top_groups(v).into_iter().map(|p| p.name).collect::<Vec<_>>();
        assert_eq!(names(a.clone()), vec!["c", "a", "b"]);
        assert_eq!(names(a), names(b));
    }
}
