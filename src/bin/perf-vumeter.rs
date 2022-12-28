// bin/perf-vumeter.rs

use anyhow::bail;
use log::*;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::{thread, time};
use structopt::StructOpt;

use perf_vumeter::*;

const MAX_DELTA: i16 = 96;
const NET_ZERO: f64 = 108.0;

const NET_RMINUS: f64 = NET_ZERO - 28.0;
const NET_FMINUS: f64 = 0.83;

const NET_RPLUS: f64 = 255.0 - NET_ZERO;
const NET_FPLUS: f64 = 0.43;

fn main() -> anyhow::Result<()> {
    let opts = OptsCommon::from_args();
    opts.start_pgm(env!("CARGO_BIN_NAME"));

    info!("Opening serial port {}", &opts.port);
    let mut ser = OpenOptions::new().read(true).write(true).open(&opts.port)?;

    info!("Vu sez hi (:");
    hello(&mut ser)?;

    let mut cpustats = CpuStats::new()?;
    let n_cpu = cpustats.n_cpu();
    let mut rx = IfStats::new(&opts.interface, IfCounter::Rx)?;
    let mut tx = IfStats::new(&opts.interface, IfCounter::Tx)?;
    let mut diskstats = DiskStats::new()?;

    let mut elapsed_ns = 0;
    let sleep_ns: u32 = 1_000_000_000 / (opts.samplerate as u32);

    info!("Starting measure loop");
    loop {
        thread::sleep(time::Duration::new(0, sleep_ns - elapsed_ns));
        let start = time::Instant::now();

        // CPU stats + gauge
        // Note: cpu_rates[0] is total/summary, the rest are sorted largest first
        let cpu_rates = cpustats.cpurates()?;
        let mut cpu_gauge = if n_cpu >= 2 {
            (cpu_rates[1] + cpu_rates[2]) / 2.0
        } else {
            cpu_rates[1]
        };

        if n_cpu >= 6 {
            cpu_gauge += (cpu_rates[3] + cpu_rates[4]) / 2.0;
            cpu_gauge += (cpu_rates[5] + cpu_rates[6]) / 3.0;
        } else if n_cpu >= 4 {
            cpu_gauge += (cpu_rates[3] + cpu_rates[4]) * 0.80;
        } else {
            cpu_gauge *= 2.56;
        }
        debug!(
            "CPU gauge: {cpu_gauge:.1} sum: {sum:.1} -- {list}",
            sum = cpu_rates[0],
            list = cpu_rates[1..]
                .iter()
                .map(|a| format!("{a:.1}"))
                .collect::<Vec<String>>()
                .join(" ")
                .as_str()
        );
        set_vu(&mut ser, 1, cpu_gauge as i16)?;

        // DISK stats + gauge
        let disk_rates = diskstats.diskrates()?;
        let disk_gauge = 256.0 * disk_rates[0] / 200_000.0;
        debug!("DISK gauge: {disk_gauge:.1} rates: {disk_rates:?}");
        set_vu(&mut ser, 2, disk_gauge as i16)?;

        // NET stats + gauge
        let rx_rate = rx.bitrate()?;
        let tx_rate = tx.bitrate()?;
        let rate = rx_rate - tx_rate;

        // special olympics for -120...240 MW meter
        let range = if rate >= 0 {
            NET_RPLUS * NET_FPLUS
        } else {
            NET_RMINUS * NET_FMINUS
        };

        let net_gauge = NET_ZERO + range * (((rate as f64) / 1_000_000.0) / (opts.max_mbps as f64));
        debug!(
            "NET gauge: {net_gauge:.1} rx: {rx} kbps, tx: {tx} kbps, rate: {rate} kbps",
            rx = rx_rate / 1000,
            tx = tx_rate / 1000,
            rate = rate / 1000,
        );
        set_vu(&mut ser, 3, net_gauge as i16)?;

        // keep the sample rate from drifting
        elapsed_ns = start.elapsed().as_nanos() as u32;
    }
}

const CHANNELS_NUM: usize = 192; // Remember: channel cmd byte has offset 0x30

fn set_vu(ser: &mut File, channel: u8, mut gauge: i16) -> anyhow::Result<()> {
    static mut LAST_VAL: [i16; CHANNELS_NUM] = [0; CHANNELS_NUM];

    let ch_i = channel as usize;
    if ch_i >= CHANNELS_NUM {
        bail!(
            "Channel number too large: {ch_i} (maximum {}",
            CHANNELS_NUM - 1
        );
    }

    gauge = gauge.clamp(28, 255);

    // do some smoothing -- only move the gauge MAX_DELTA at once
    let delta = unsafe { gauge - LAST_VAL[ch_i] };
    let delta_sig = delta.signum();
    let delta_trunc = delta.abs().min(MAX_DELTA);
    let new_value = unsafe { LAST_VAL[ch_i] + delta_sig * delta_trunc };
    unsafe {
        LAST_VAL[ch_i] = new_value;
    }

    let cmd_buf: [u8; 4] = [0xFD, 0x02, 0x30 + channel, new_value as u8];
    Ok(ser.write_all(&cmd_buf)?)
}

fn hello(ser: &mut File) -> anyhow::Result<()> {
    for i in (0i16..=255)
        .chain((128..=255).rev())
        .chain(128..=255)
        .chain((0..=255).rev())
    {
        for c in 1u8..=3 {
            set_vu(ser, c, i)?;
        }
        thread::sleep(time::Duration::new(0, 3_000_000));
    }
    Ok(())
}
// EOF
