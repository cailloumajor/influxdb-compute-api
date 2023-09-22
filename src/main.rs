use anyhow::Context as _;
use axum::Server;
use clap::Parser;
use clap_verbosity_flag::{InfoLevel, Verbosity};
use futures_util::StreamExt;
use influxdb_compute_api::CommonArgs;
use signal_hook::consts::TERM_SIGNALS;
use signal_hook::low_level::signal_name;
use signal_hook_tokio::Signals;
use tracing::{error, info, info_span, instrument, Instrument};
use tracing_log::LogTracer;

mod channel;
mod config_api;
mod headers;
mod http_api;
mod influxdb;
mod level_filter;
mod production_objective;
mod time;

use level_filter::VerbosityLevelFilter;

#[derive(Parser)]
struct Args {
    #[command(flatten)]
    common: CommonArgs,

    #[command(flatten)]
    config_api: config_api::Config,

    #[command(flatten)]
    influxdb: influxdb::Config,

    #[command(flatten)]
    verbose: Verbosity<InfoLevel>,
}

#[instrument(skip_all)]
async fn handle_signals(signals: Signals) {
    let mut signals_stream = signals.map(|signal| signal_name(signal).unwrap_or("unknown"));
    info!(status = "started");
    if let Some(signal) = signals_stream.next().await {
        info!(msg = "received signal", reaction = "shutting down", signal);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_max_level(VerbosityLevelFilter::from(&args.verbose))
        .init();

    LogTracer::init_with_filter(args.verbose.log_level_filter())?;

    let http_client = reqwest::Client::new();

    let config_api_client = config_api::Client::new(&args.config_api, http_client.clone());
    let (common_config_channel, common_config_task) = config_api_client.handle_common_config();
    let (partner_config_channel, partner_config_task) = config_api_client.handle_partner_config();

    let influxdb_client = influxdb::Client::new(&args.influxdb, http_client);
    let (health_channel, health_task) = influxdb_client.handle_health();
    let (timeline_channel, timeline_task) = influxdb_client.handle_timeline();
    let (performance_channel, performance_task) = influxdb_client.handle_performance();

    let production_objective = production_objective::ProductionObjective;
    let (shift_objective_channel, shift_objective_task) =
        production_objective.handle_shift_objective();
    let (week_objective_channel, week_objective_task) =
        production_objective.handle_week_objective();

    let signals = Signals::new(TERM_SIGNALS).context("error registering termination signals")?;
    let signals_handle = signals.handle();

    let app = http_api::app(http_api::AppState {
        health_channel,
        common_config_channel,
        partner_config_channel,
        timeline_channel,
        performance_channel,
        shift_objective_channel,
        week_objective_channel,
    });
    async move {
        info!(addr = %args.common.listen_address, msg = "start listening");
        if let Err(err) = Server::bind(&args.common.listen_address)
            .serve(app.into_make_service())
            .with_graceful_shutdown(handle_signals(signals))
            .await
        {
            error!(kind = "HTTP server", %err);
        }
        info!(status = "terminating");
    }
    .instrument(info_span!("http_server_task"))
    .await;

    signals_handle.close();

    tokio::try_join!(
        common_config_task,
        partner_config_task,
        health_task,
        timeline_task,
        performance_task,
        shift_objective_task,
        week_objective_task,
    )
    .context("error joining tasks")?;

    Ok(())
}
