// stats.rs

use anyhow::anyhow;
use log::*;
use std::io::{self, BufRead};
use std::{collections::HashMap, fs::File, path::Path};
use std::{fmt, time};

const CPU_JIFF: f64 = 100.0;

#[derive(Debug)]
pub enum IfCounter {
    Rx,
    Tx,
}
impl fmt::Display for IfCounter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                IfCounter::Rx => "rx_bytes",
                IfCounter::Tx => "tx_bytes",
            }
        )
    }
}

#[derive(Debug)]
pub struct IfStats {
    pub iface: String,
    pub dir: IfCounter,
    fn_stats: String,
    prev_ts: time::Instant,
    prev_cnt: i64,
}
impl IfStats {
    pub fn new<S: AsRef<str>>(iface: S, dir: IfCounter) -> anyhow::Result<Self> {
        let fn_stats = format!("/sys/class/net/{}/statistics/{}", iface.as_ref(), dir);
        let prev_cnt = Self::read_number(&fn_stats)?;
        Ok(Self {
            iface: iface.as_ref().to_string(),
            dir,
            fn_stats,
            prev_ts: time::Instant::now(),
            prev_cnt,
        })
    }
    pub fn bitrate(&mut self) -> anyhow::Result<i64> {
        let us = self.prev_ts.elapsed().as_micros();
        self.prev_ts = time::Instant::now();
        let cnt = Self::read_number(&self.fn_stats)?;
        let rate = ((8 * (cnt - self.prev_cnt)) as f64 / (us as f64 / 1_000_000.0)) as i64;
        self.prev_cnt = cnt;
        Ok(rate)
    }
    fn read_number<P>(filename: P) -> anyhow::Result<i64>
    where
        P: AsRef<Path>,
    {
        let file = File::open(filename)?;
        let mut lines = io::BufReader::new(file).lines();
        if let Some(line) = lines.next() {
            let n = line?;
            return Ok(n.parse::<i64>()?);
        }
        Err(anyhow!("empty"))
    }
}

#[derive(Debug)]
pub struct CpuStats {
    prev_ts: time::Instant,
    prev_idle: Vec<i64>,
}
impl CpuStats {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            prev_ts: time::Instant::now(),
            prev_idle: Self::read_cpuidle()?,
        })
    }
    pub fn cpurates(&mut self) -> anyhow::Result<Vec<f64>> {
        let us = self.prev_ts.elapsed().as_micros();
        self.prev_ts = time::Instant::now();

        let idle = Self::read_cpuidle()?;
        let factor = 100.0 * 1_000_000.0 / (us as f64 * CPU_JIFF);
        let n_cpu = (idle.len() - 1) as f64;

        let mut rates = Vec::with_capacity(idle.len());
        for (i, r) in idle.iter().enumerate() {
            let factor2 = if i == 0 { n_cpu } else { 1.0 };
            // cpu usage is 100% minus idle.
            let rate = 100.0 - ((factor * (r - self.prev_idle[i]) as f64) / factor2);
            rates.push(rate);
        }
        // Rust refuses to just sort() f64, because NaN etc.
        rates[1..].sort_by(|a, b| b.partial_cmp(a).unwrap());
        self.prev_idle = idle;
        Ok(rates)
    }
    pub fn n_cpu(&self) -> usize {
        self.prev_idle.len() - 1
    }
    fn read_cpuidle() -> anyhow::Result<Vec<i64>> {
        let file = File::open("/proc/stat")?;
        let mut cpu_idle = Vec::with_capacity(32);
        for line in io::BufReader::new(file).lines() {
            let line = line?;
            let items = line.split_ascii_whitespace().collect::<Vec<&str>>();
            if !items[0].starts_with("cpu") {
                break;
            }
            cpu_idle.push(items[4].parse::<i64>()?);
        }
        Ok(cpu_idle)
    }
}

#[derive(Debug)]
pub struct DiskStats {
    prev_ts: time::Instant,
    prev_stats: HashMap<String, (i64, i64)>,
}
impl DiskStats {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            prev_ts: time::Instant::now(),
            prev_stats: Self::read_diskstats()?,
        })
    }
    pub fn diskrates(&mut self) -> anyhow::Result<Vec<f64>> {
        let us = self.prev_ts.elapsed().as_micros();
        self.prev_ts = time::Instant::now();

        let stats = Self::read_diskstats()?;
        let mut rates = Vec::with_capacity(stats.len());

        for (k, v) in &stats {
            if self.prev_stats.get(k).is_none() {
                // did not have this key last time!
                continue;
            }
            let sect_rd = v.0 - self.prev_stats.get(k).unwrap().0;
            let sect_wrt = v.1 - self.prev_stats.get(k).unwrap().1;
            rates.push((sect_rd + sect_wrt) as f64 * 1_000_000.0 / us as f64);
        }
        // Rust refuses to just sort() f64, because NaN, Inf etc.
        rates.sort_by(|a, b| b.partial_cmp(a).unwrap());
        self.prev_stats = stats;
        Ok(rates)
    }
    // https://www.kernel.org/doc/Documentation/ABI/testing/procfs-diskstats
    fn read_diskstats() -> anyhow::Result<HashMap<String, (i64, i64)>> {
        let file = File::open("/proc/diskstats")?;
        let mut stats = HashMap::with_capacity(32);
        for line in io::BufReader::new(file).lines() {
            let line = line?;
            let items = line.split_ascii_whitespace().collect::<Vec<&str>>();
            let devname = items[2];
            // collect sectors read and sectors written from "sd?" and "nvme???"
            if devname.starts_with("sd") && devname.len() == 3
                || devname.starts_with("nvme") && devname.len() == 7
            {
                let sect_rd = items[5].parse::<i64>()?;
                let sect_wrt = items[9].parse::<i64>()?;
                stats.insert(devname.into(), (sect_rd, sect_wrt));
            }
        }
        debug!("{:?}", stats);
        Ok(stats)
    }
}
// EOF
