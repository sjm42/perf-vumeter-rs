// main.rs

use log::*;
use serialport::{DataBits, FlowControl, Parity, StopBits};
use std::{error::Error, thread, time};
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

fn main() -> Result<(), Box<dyn Error>> {
    let opts = OptsCommon::from_args();
    start_pgm(&opts, "Bitrate VU meter");

    let mut ser = serialport::new(&opts.port, 9600)
        .parity(Parity::None)
        .data_bits(DataBits::Eight)
        .stop_bits(StopBits::One)
        .flow_control(FlowControl::None)
        .open()?;
    let mut i = 0u8;
    let mut up = true;
    let mut cmd_buf: [u8; 4] = [0x00, 0xFF, 0x01, 0x00];
    loop {
        cmd_buf[3] = i;
        ser.write_all(&cmd_buf)?;
        if up {
            i += 1;
            if i == 255 {
                up = false;
            }
        } else {
            i -= 1;
            if i == 0 {
                up = true;
            }
        }
        thread::sleep(time::Duration::new(0, 10_000_000));
    }
    // Ok(())
}

// EOF
