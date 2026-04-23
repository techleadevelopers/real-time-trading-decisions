from __future__ import annotations

import numpy as np
import pandas as pd


FEATURE_COLUMNS = [
    "slippage_error",
    "fill_efficiency_score",
    "latency_penalty",
    "orderbook_imbalance_multilevel",
    "trade_velocity",
    "volatility_micro_spike",
    "snapshot_staleness_score",
    "fill_probability_estimate",
    "competition_density_index",
    "execution_fragility_index",
    "inclusion_drift_score",
    "market_microstructure_stress",
    "regime_instability_score",
    "orderbook_depth_shock",
    "spread_change_rate",
    "adverse_selection_score",
    "mempool_pressure",
    "competition_density",
    "execution_delay_ms",
    "volatility_regime_score",
]


def clip01(value):
    return np.clip(value, 0.0, 1.0)


def safe_col(df: pd.DataFrame, name: str, default: float = 0.0) -> pd.Series:
    if name in df:
        return pd.to_numeric(df[name], errors="coerce").fillna(default)
    return pd.Series(default, index=df.index, dtype=float)


def robust_z(series: pd.Series, window: int = 200) -> pd.Series:
    values = pd.to_numeric(series, errors="coerce").fillna(0.0)
    median = values.rolling(window, min_periods=20).median()
    mad = (values - median).abs().rolling(window, min_periods=20).median()
    z = (values - median) / (1.4826 * mad + 1e-9)
    return z.replace([np.inf, -np.inf], 0.0).fillna(0.0)


def bounded_from_z(z: pd.Series, scale: float = 3.0) -> pd.Series:
    return pd.Series(clip01((z / scale + 1.0) * 0.5), index=z.index)


def generate_execution_features(raw: pd.DataFrame) -> pd.DataFrame:
    """
    Build bounded execution-quality features from structured logs.

    The function does not create trading direction labels. Markouts are used only
    as post-trade quality evidence.
    """
    df = raw.copy()
    if df.empty:
        return df

    expected_slip = safe_col(df, "expected_slippage_bps")
    realized_slip = safe_col(df, "realized_slippage_bps")
    requested = safe_col(df, "requested_size", 1.0).replace(0.0, np.nan)
    filled = safe_col(df, "filled_size")
    delay_ms = safe_col(df, "execution_delay_ms")
    tau_ms = safe_col(df, "latency_tau_ms", 180.0).clip(lower=20.0)

    df["slippage_error"] = (realized_slip - expected_slip).clip(-100.0, 100.0)
    delay_penalty = 1.0 - np.exp(-delay_ms.clip(lower=0.0) / tau_ms)
    raw_fill_efficiency = (filled / requested).fillna(0.0)
    price_penalty = clip01(df["slippage_error"].clip(lower=0.0) / 25.0)
    df["fill_efficiency_score"] = clip01(
        raw_fill_efficiency
        * (1.0 - 0.5 * delay_penalty)
        * (1.0 - 0.5 * price_penalty)
    )
    df["latency_penalty"] = clip01(delay_penalty)

    bid_volume = safe_col(df, "bid_volume_total")
    ask_volume = safe_col(df, "ask_volume_total")
    top_bid = safe_col(df, "best_bid_volume")
    top_ask = safe_col(df, "best_ask_volume")
    if "orderbook_imbalance_multilevel" in df:
        imbalance = safe_col(df, "orderbook_imbalance_multilevel")
    else:
        total_depth = bid_volume + ask_volume + 1e-9
        top_total = top_bid + top_ask + 1e-9
        depth_imbalance = (bid_volume - ask_volume) / total_depth
        top_imbalance = (top_bid - top_ask) / top_total
        imbalance = 0.65 * depth_imbalance + 0.35 * top_imbalance
    df["orderbook_imbalance_multilevel"] = pd.Series(imbalance, index=df.index).clip(-1.0, 1.0)

    df["trade_velocity"] = bounded_from_z(
        robust_z(safe_col(df, "trade_velocity", safe_col(df, "trade_frequency")))
    )
    df["volatility_micro_spike"] = clip01(
        safe_col(df, "volatility_micro_spike", safe_col(df, "volatility_spike_score"))
    )
    staleness_ms = safe_col(df, "snapshot_staleness_ms", safe_col(df, "data_latency_ms"))
    stale_tau = safe_col(df, "staleness_tau_ms", 120.0).clip(lower=10.0)
    df["snapshot_staleness_score"] = clip01(1.0 - np.exp(-staleness_ms.clip(lower=0.0) / stale_tau))
    df["fill_probability_estimate"] = clip01(
        safe_col(df, "fill_probability_estimate", safe_col(df, "fill_probability", 0.5))
    )

    burst = bounded_from_z(robust_z(safe_col(df, "tx_arrival_rate")))
    clustered = clip01(safe_col(df, "router_pool_clustering_density"))
    fail_rate = clip01(safe_col(df, "recent_execution_failure_rate"))
    outbid_rate = clip01(safe_col(df, "outbid_rate"))
    df["competition_density_index"] = clip01(
        0.35 * clip01(safe_col(df, "mempool_pressure"))
        + 0.25 * burst
        + 0.20 * clustered
        + 0.20 * np.maximum(fail_rate, outbid_rate)
    )

    reject_rate = clip01(safe_col(df, "recent_rejection_rate"))
    replace_rate = clip01(safe_col(df, "replace_rate"))
    partial_rate = clip01(safe_col(df, "partial_fill_rate"))
    latency_jitter = bounded_from_z(robust_z(safe_col(df, "execution_latency_jitter_ms")))
    df["execution_fragility_index"] = clip01(
        0.30 * reject_rate
        + 0.20 * replace_rate
        + 0.20 * partial_rate
        + 0.30 * latency_jitter
    )

    hist_inclusion = clip01(safe_col(df, "historical_inclusion_rate", 1.0))
    recent_inclusion = clip01(safe_col(df, "recent_inclusion_rate", 1.0))
    df["inclusion_drift_score"] = clip01(hist_inclusion - recent_inclusion)

    spread_widening = clip01(safe_col(df, "spread_widening_score"))
    depth_shock = clip01(safe_col(df, "orderbook_depth_shock"))
    imbalance_shift = clip01(safe_col(df, "imbalance_shift_score"))
    vol_spike = clip01(safe_col(df, "volatility_spike_score"))
    df["market_microstructure_stress"] = clip01(
        0.30 * spread_widening
        + 0.30 * depth_shock
        + 0.20 * imbalance_shift
        + 0.20 * vol_spike
    )

    df["regime_instability_score"] = bounded_from_z(
        robust_z(safe_col(df, "regime_transition_count"), window=100)
    )
    df["orderbook_depth_shock"] = depth_shock
    df["spread_change_rate"] = clip01(safe_col(df, "spread_change_rate"))

    markout_100 = safe_col(df, "markout_100ms")
    markout_500 = safe_col(df, "markout_500ms")
    adverse = -1.0 * (0.65 * markout_100 + 0.35 * markout_500)
    adverse_scale = safe_col(df, "adverse_markout_scale", 10.0).clip(lower=1e-9)
    df["adverse_selection_score"] = clip01(adverse.clip(lower=0.0) / adverse_scale)

    df["mempool_pressure"] = clip01(safe_col(df, "mempool_pressure"))
    df["competition_density"] = clip01(safe_col(df, "competition_density"))
    df["execution_delay_ms"] = delay_ms.clip(lower=0.0)
    df["volatility_regime_score"] = clip01(safe_col(df, "volatility_regime_score", 0.5))

    for col in FEATURE_COLUMNS:
        if col not in df:
            df[col] = 0.0
        df[col] = (
            pd.to_numeric(df[col], errors="coerce")
            .replace([np.inf, -np.inf], 0.0)
            .fillna(0.0)
        )
    return df


def feature_matrix(df: pd.DataFrame) -> np.ndarray:
    prepared = generate_execution_features(df)
    return prepared[FEATURE_COLUMNS].astype(float).values
