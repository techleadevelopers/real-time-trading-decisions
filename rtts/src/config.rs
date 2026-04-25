use clap::{Parser, ValueEnum};
use std::time::Duration;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Exchange {
    Binance,
    Mock,
}

#[derive(Clone, Debug, Parser)]
#[command(name = "scalp-sniper-rtts")]
pub struct Config {
    #[arg(long, env = "RTTS_EXCHANGE", default_value = "mock")]
    pub exchange: Exchange,
    #[arg(long, env = "RTTS_SYMBOL", default_value = "BTCUSDT")]
    pub symbol: String,
    #[arg(long, env = "RTTS_CAPITAL", default_value_t = 10_000.0)]
    pub capital: f64,
    #[arg(long, env = "RTTS_MAX_RISK_PCT", default_value_t = 0.005)]
    pub max_risk_pct: f64,
    #[arg(long, env = "RTTS_DAILY_DD_PCT", default_value_t = 0.02)]
    pub max_daily_drawdown_pct: f64,
    #[arg(long, env = "RTTS_BASE_ORDER_USD", default_value_t = 25.0)]
    pub base_order_usd: f64,
    #[arg(long, env = "RTTS_MAX_ENTRIES", default_value_t = 4)]
    pub max_entries: u32,
    #[arg(long, env = "RTTS_STOP_LOSS_BPS", default_value_t = 25.0)]
    pub stop_loss_bps: f64,
    #[arg(long, env = "RTTS_MAX_DATA_AGE_MS", default_value_t = 250)]
    pub max_data_age_ms: u64,
    #[arg(long, env = "RTTS_MAX_DECISION_LATENCY_US", default_value_t = 1_500)]
    pub max_decision_latency_us: u64,
    #[arg(long, env = "RTTS_MAX_EXECUTION_LATENCY_US", default_value_t = 8_000)]
    pub max_execution_latency_us: u64,
    #[arg(long, env = "RTTS_MAX_CONSECUTIVE_LOSSES", default_value_t = 3)]
    pub max_consecutive_losses: u32,
    #[arg(long, env = "RTTS_CHANNEL_CAP", default_value_t = 4096)]
    pub channel_capacity: usize,
    #[arg(long, env = "RTTS_WINDOW_MS", default_value_t = 500)]
    pub window_ms: u64,
    #[arg(long, env = "RTTS_METRICS_ADDR", default_value = "127.0.0.1:9898")]
    pub metrics_addr: String,
    #[arg(long, env = "RTTS_CONTROL_PLANE_HTTP", default_value = "http://127.0.0.1:8088")]
    pub control_plane_http: String,
    #[arg(long, env = "RTTS_CONTROL_PLANE_WS", default_value = "ws://127.0.0.1:8088/ws")]
    pub control_plane_ws: String,
    #[arg(long, env = "RTTS_MAX_CANCEL_PER_ORDER", default_value_t = 2)]
    pub max_cancel_per_order: u32,
    #[arg(long, env = "RTTS_MAX_REPLACE_PER_ORDER", default_value_t = 3)]
    pub max_replace_per_order: u32,
    #[arg(long, env = "RTTS_EXECUTION_ACTION_COOLDOWN_MS", default_value_t = 40)]
    pub execution_action_cooldown_ms: u64,
    #[arg(long, env = "RTTS_QUEUE_REPLACE_VOLUME_FACTOR", default_value_t = 1.35)]
    pub queue_replace_volume_factor: f64,
    #[arg(long, env = "RTTS_MIN_FILL_PROBABILITY", default_value_t = 0.28)]
    pub min_fill_probability: f64,
}

impl Config {
    #[inline]
    pub fn window(&self) -> Duration {
        Duration::from_millis(self.window_ms)
    }
}
