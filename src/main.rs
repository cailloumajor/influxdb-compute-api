use anyhow::Ok;
use clap::Parser;
use clap_verbosity_flag::{InfoLevel, Verbosity};

use influxdb_compute_api::CommonArgs;

mod influxdb;
mod level_filter;

use level_filter::VerbosityLevelFilter;
use tracing_log::LogTracer;

#[derive(Parser)]
struct Args {
    #[command(flatten)]
    common: CommonArgs,

    #[command(flatten)]
    verbose: Verbosity<InfoLevel>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_max_level(VerbosityLevelFilter::from(&args.verbose))
        .init();

    LogTracer::init_with_filter(args.verbose.log_level_filter())?;

    Ok(())
}
