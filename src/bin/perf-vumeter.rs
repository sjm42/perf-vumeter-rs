// bin/perf-vumeter.rs

use log::*;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::{cmp, thread, time};
use structopt::StructOpt;

use perf_vumeter::*;

const MAX_DELTA: f64 = 96.0;

fn main() -> anyhow::Result<()> {
    let opts = OptsCommon::from_args();
    start_pgm(&opts, "Performance VU meter");

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
        set_vu(&mut ser, 1, cpu_gauge)?;

        // DISK stats + gauge
        let disk_rates = diskstats.diskrates()?;
        debug!("DISK rates: {disk_rates:?}");
        let disk_gauge = 256.0 * disk_rates[0] / 200_000.0;
        set_vu(&mut ser, 2, disk_gauge)?;

        // NET stats + gauge
        let rx_rate = rx.bitrate()?;
        let tx_rate = tx.bitrate()?;
        let rate = cmp::max(rx_rate, tx_rate);
        let net_gauge = 256.0 * (((rate as f64) / 1_000_000.0) / (opts.max_mbps as f64));
        debug!(
            "NET rx: {rx} kbps, tx: {tx} kbps, gauge: {net_gauge}",
            rx = rx_rate / 1000,
            tx = tx_rate / 1000
        );
        set_vu(&mut ser, 3, net_gauge)?;

        // keep the sample rate from drifting
        elapsed_ns = start.elapsed().as_nanos() as u32;
    }
}

// only allow values between 0.0 ... 255.0
fn set_vu(ser: &mut File, channel: u8, mut gauge: f64) -> anyhow::Result<()> {
    static mut LAST_VAL: [u8; 256] = [0; 256];
    let ch_i = channel as usize;
    if gauge > 255.0 {
        gauge = 255.0;
    }
    if gauge < 0.0 {
        gauge = 0.0;
    }

    // do some magic smoothing -- only move the gauge MAX_DELTA at once
    let delta = unsafe { (LAST_VAL[ch_i] as f64 - gauge).abs() };
    let delta_pos = unsafe { (gauge - LAST_VAL[ch_i] as f64).is_sign_positive() };
    let delta_trunc = delta.min(MAX_DELTA) as u8;
    let value = unsafe {
        if delta_pos {
            LAST_VAL[ch_i] + delta_trunc
        } else {
            LAST_VAL[ch_i] - delta_trunc
        }
    };
    unsafe {
        LAST_VAL[ch_i] = value;
    }

    let cmd_buf: [u8; 4] = [0xFD, 0x02, 0x30 + channel, value];

    Ok(ser.write_all(&cmd_buf)?)
}

fn hello(ser: &mut File) -> anyhow::Result<()> {
    for i in (0..=255u8)
        .chain((128..=255).rev())
        .chain(128..=255)
        .chain((0..=255).rev())
    {
        for c in 1..=3u8 {
            set_vu(ser, c, i as f64)?;
        }
        thread::sleep(time::Duration::new(0, 3_000_000));
    }
    Ok(())
}
// EOF
