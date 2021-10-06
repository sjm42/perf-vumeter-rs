// main.rs

use anyhow::anyhow;
use log::*;
use serialport::{DataBits, FlowControl, Parity, SerialPort, StopBits};
use std::io::{self, BufRead};
use std::{cmp, fmt, thread, time};
use std::{collections::HashMap, fs::File, path::Path};
use structopt::StructOpt;

const CPU_JIFF: f64 = 100.0;

#[derive(Debug, Default, StructOpt)]
pub struct OptsCommon {
    #[structopt(short, long)]
    pub debug: bool,
    #[structopt(short, long)]
    pub trace: bool,
    #[structopt(short, long, default_value = "/dev/ttyUSB0")]
    pub port: String,
    #[structopt(short, long, default_value = "br0")]
    pub interface: String,
    #[structopt(short, long, default_value = "2")]
    pub samplerate: u16,
    #[structopt(short, long, default_value = "100")]
    pub max_mbps: u16,
}
impl OptsCommon {
    fn get_loglevel(&self) -> LevelFilter {
        if self.trace {
            LevelFilter::Trace
        } else if self.debug {
            LevelFilter::Debug
        } else {
            LevelFilter::Info
        }
    }
}

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
            if let None = self.prev_stats.get(k) {
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

fn start_pgm(c: &OptsCommon, desc: &str) {
    env_logger::Builder::new()
        .filter_level(c.get_loglevel())
        .format_timestamp_secs()
        .init();
    info!("Starting up {}...", desc);
    debug!("Git branch: {}", env!("GIT_BRANCH"));
    debug!("Git commit: {}", env!("GIT_COMMIT"));
    debug!("Source timestamp: {}", env!("SOURCE_TIMESTAMP"));
    debug!("Compiler version: {}", env!("RUSTC_VERSION"));
}

// only use values between 0.0 ... 255.0
fn set_vu(ser: &mut dyn SerialPort, channel: u8, mut gauge: f64) -> anyhow::Result<()> {
    if gauge > 255.0 {
        gauge = 255.0;
    }
    if gauge < 0.0 {
        gauge = 0.0;
    }
    let value = gauge as u8;
    let cmd_buf: [u8; 4] = [0x00, 0xFF, channel, value];
    Ok(ser.write_all(&cmd_buf)?)
}

fn hello(ser: &mut dyn SerialPort) -> anyhow::Result<()> {
    for i in (0..=255u8)
        .chain((128..=255).rev())
        .chain(128..=255)
        .chain((0..=255).rev())
    {
        for c in 1..=3u8 {
            set_vu(ser, c, i as f64)?;
        }
        thread::sleep(time::Duration::new(0, 5_000_000));
    }
    Ok(())
}

#[allow(unreachable_code)]
fn main() -> anyhow::Result<()> {
    let opts = OptsCommon::from_args();
    start_pgm(&opts, "Bitrate VU meter");

    let mut ser = serialport::new(&opts.port, 115200)
        .parity(Parity::None)
        .data_bits(DataBits::Eight)
        .stop_bits(StopBits::One)
        .flow_control(FlowControl::None)
        .open()?;

    hello(&mut *ser)?;

    let mut cpustats = CpuStats::new()?;
    let n_cpu = cpustats.n_cpu();
    let mut rx = IfStats::new(&opts.interface, IfCounter::Rx)?;
    let mut tx = IfStats::new(&opts.interface, IfCounter::Tx)?;
    let mut diskstats = DiskStats::new()?;

    let mut elapsed_ns = 0;
    let sleep_ns: u32 = 1_000_000_000 / (opts.samplerate as u32);

    loop {
        thread::sleep(time::Duration::new(0, sleep_ns - elapsed_ns));
        let now = time::Instant::now();

        // CPU stats + gauge
        // Note: cpu_rates[0] is total/summary, the rest are sorted largest first
        let cpu_rates = cpustats.cpurates()?;
        let mut cpu_gauge;
        if n_cpu >= 2 {
            cpu_gauge = (cpu_rates[1] + cpu_rates[2]) / 2.0;
        } else {
            cpu_gauge = cpu_rates[1];
        }

        if n_cpu >= 6 {
            cpu_gauge += (cpu_rates[3] + cpu_rates[4]) / 2.0;
            cpu_gauge += (cpu_rates[5] + cpu_rates[6]) / 3.0;
        } else if n_cpu >= 4 {
            cpu_gauge += (cpu_rates[3] + cpu_rates[4]) * 0.80;
        } else {
            cpu_gauge *= 2.56;
        }
        debug!(
            "CPU gauge: {:.1} sum: {:.1} -- {}",
            cpu_gauge,
            cpu_rates[0],
            cpu_rates[1..]
                .iter()
                .map(|a| format!("{:.1}", a))
                .collect::<Vec<String>>()
                .join(" ")
                .as_str()
        );
        set_vu(&mut *ser, 1, cpu_gauge)?;

        // DISK stats + gauge
        let disk_rates = diskstats.diskrates()?;
        debug!("DISK rates: {:?}", disk_rates);
        let disk_gauge = 256.0 * disk_rates[0] / 200_000.0;
        debug!("DISK gauge: {:.1}", disk_gauge);
        set_vu(&mut *ser, 2, disk_gauge)?;

        // NET stats + gauge
        let rx_rate = rx.bitrate()?;
        let tx_rate = tx.bitrate()?;
        let rate = cmp::max(rx_rate, tx_rate);
        let net_gauge = 256.0 * (((rate as f64) / 1_000_000.0) / (opts.max_mbps as f64));
        debug!(
            "NET rx: {} kbps, tx: {} kbps, gauge: {}",
            rx_rate / 1000,
            tx_rate / 1000,
            net_gauge
        );
        set_vu(&mut *ser, 3, net_gauge)?;

        elapsed_ns = now.elapsed().as_nanos() as u32;
    }
}
// EOF
