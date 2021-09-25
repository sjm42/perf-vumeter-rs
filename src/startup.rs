// startup.rs

use log::*;
use std::{error::Error, fmt::Debug};

use structopt::StructOpt;

#[derive(Clone, Debug, StructOpt)]
pub struct OptsCommon {
    #[structopt(short, long)]
    pub debug: bool,
    #[structopt(short, long)]
    pub trace: bool,
    #[structopt(short, long, default_value = "127.0.0.1:8080")]
    pub listen: String,
    #[structopt(short, long, default_value = "coap://127.0.0.1/")]
    pub coap_url: String,
}
impl OptsCommon {
    pub fn finish(&mut self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
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

// EOF
