// main.rs

use anyhow::anyhow;
use log::*;
use serialport::{DataBits, FlowControl, Parity, StopBits};
use std::io::{self, BufRead};
use std::{cmp, fmt, thread, time};
use std::{fs::File, path::Path};
use structopt::StructOpt;

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
    #[structopt(short, long, default_value = "5")]
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
pub enum Cnt {
    Rx,
    Tx,
}
impl fmt::Display for Cnt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Cnt::Rx => "rx_bytes",
                Cnt::Tx => "tx_bytes",
            }
        )
    }
}

#[derive(Debug)]
pub struct Stats {
    pub iface: String,
    pub dir: Cnt,
    fn_stats: String,
    prev_ts: time::Instant,
    prev_cnt: i64,
}
impl Stats {
    pub fn new<S: AsRef<str>>(iface: S, dir: Cnt) -> anyhow::Result<Self> {
        let fn_stats = format!("/sys/class/net/{}/statistics/{}", iface.as_ref(), dir);
        let prev_cnt = read_number(&fn_stats)?;
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
        let cnt = read_number(&self.fn_stats)?;
        let rate = ((8 * (cnt - self.prev_cnt)) as f64 / (us as f64 / 1_000_000.0)) as i64;
        self.prev_ts = time::Instant::now();
        self.prev_cnt = cnt;
        Ok(rate)
    }
}

pub fn read_number<P>(filename: P) -> anyhow::Result<i64>
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

pub fn start_pgm(c: &OptsCommon, desc: &str) {
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

fn main() -> anyhow::Result<()> {
    let opts = OptsCommon::from_args();
    start_pgm(&opts, "Bitrate VU meter");

    let mut ser = serialport::new(&opts.port, 9600)
        .parity(Parity::None)
        .data_bits(DataBits::Eight)
        .stop_bits(StopBits::One)
        .flow_control(FlowControl::None)
        .open()?;
    let mut cmd_buf: [u8; 4] = [0x00, 0xFF, 0x01, 0x00];
    let mut rx = Stats::new(&opts.interface, Cnt::Rx)?;
    let mut tx = Stats::new(&opts.interface, Cnt::Tx)?;
    let mut elapsed_ns = 0;
    let sleep_ns: u32 = 1_000_000_000 / (opts.samplerate as u32);

    loop {
        // sleep 200ms
        thread::sleep(time::Duration::new(0, sleep_ns - elapsed_ns));

        let now = time::Instant::now();
        let rx_rate = rx.bitrate()?;
        let tx_rate = tx.bitrate()?;
        let rate = cmp::max(rx_rate, tx_rate);

        let mut gauge = 256.0 * (((rate as f64) / 1_000_000.0) / (opts.max_mbps as f64));
        if gauge > 255.0 {
            gauge = 255.0;
        }
        if gauge < 0.0 {
            gauge = 0.0;
        }

        let i = gauge as u8;
        cmd_buf[3] = i;
        ser.write_all(&cmd_buf)?;
        debug!(
            "rx: {} kbps, tx: {} kbps, gauge: {}",
            rx_rate / 1000,
            tx_rate / 1000,
            gauge
        );
        elapsed_ns = now.elapsed().as_nanos() as u32;
    }
    // Ok(())
}

// EOF
