use anyhow::Result;
use clap::Parser;
use scalp_sniper_rtts::{config::Config, metrics::Metrics, model_weights, pipeline};
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let cfg = Config::parse();
    model_weights::initialize(&cfg.model_weights_path)?;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).json().init();

    let metrics = Metrics::new()?;
    pipeline::run(cfg, metrics).await
}
