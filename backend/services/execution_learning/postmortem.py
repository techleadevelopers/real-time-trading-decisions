from __future__ import annotations

import numpy as np
import pandas as pd

from .features import clip01, generate_execution_features, safe_col


MARKOUT_WINDOWS = ["markout_100ms", "markout_500ms", "markout_1s", "markout_5s"]


def compute_execution_postmortem(raw: pd.DataFrame) -> pd.DataFrame:
    df = generate_execution_features(raw)
    if df.empty:
        return df

    slippage_error = safe_col(df, "slippage_error")
    slippage_scale = safe_col(df, "slippage_scale_bps", 15.0).clip(lower=1e-9)
    realized_pnl = safe_col(df, "realized_pnl")
    fees = safe_col(df, "fees")
    slippage_cost = safe_col(df, "slippage_cost")
    if "slippage_cost" not in df:
        notional = safe_col(df, "filled_notional")
        if "filled_notional" not in df:
            notional = safe_col(df, "filled_size") * safe_col(df, "fill_price")
        slippage_cost = notional * safe_col(df, "realized_slippage_bps").abs() / 10_000.0

    df["execution_adjusted_return"] = (realized_pnl - fees - slippage_cost).clip(-1_000.0, 1_000.0)
    return_scale = safe_col(df, "return_scale", 50.0).clip(lower=1e-9)
    df["realized_return_score"] = clip01(0.5 + df["execution_adjusted_return"] / (2.0 * return_scale))
    df["slippage_accuracy_score"] = clip01(1.0 - (slippage_error.clip(lower=0.0) / slippage_scale))

    markout_penalty = np.zeros(len(df), dtype=float)
    weights = {
        "markout_100ms": 0.40,
        "markout_500ms": 0.30,
        "markout_1s": 0.20,
        "markout_5s": 0.10,
    }
    markout_scale = safe_col(df, "markout_scale", 10.0).clip(lower=1e-9)
    for col, weight in weights.items():
        markout = safe_col(df, col)
        markout_penalty += weight * clip01((-markout.clip(upper=0.0)) / markout_scale)
    df["markout_adverse_penalty"] = clip01(markout_penalty)

    df["execution_quality_score"] = clip01(
        0.30 * safe_col(df, "fill_efficiency_score")
        + 0.25 * safe_col(df, "realized_return_score")
        + 0.15 * safe_col(df, "slippage_accuracy_score")
        + 0.15 * (1.0 - safe_col(df, "latency_penalty"))
        + 0.15 * (1.0 - safe_col(df, "markout_adverse_penalty"))
    )
    return df


def markout_curve_by_window(df: pd.DataFrame) -> dict[str, float]:
    if df.empty:
        return {window: 0.0 for window in MARKOUT_WINDOWS}
    return {
        window: float(pd.to_numeric(df.get(window, 0.0), errors="coerce").fillna(0.0).mean())
        for window in MARKOUT_WINDOWS
    }


def latency_penalty_curve(df: pd.DataFrame, buckets: list[int] | None = None) -> dict[str, float]:
    if buckets is None:
        buckets = [10, 25, 50, 100, 250, 500, 1000]
    scored = compute_execution_postmortem(df)
    if scored.empty:
        return {f"lte_{bucket}ms": 0.0 for bucket in buckets}

    delay = safe_col(scored, "execution_delay_ms")
    quality = safe_col(scored, "execution_quality_score")
    result: dict[str, float] = {}
    lower = 0.0
    for bucket in buckets:
        mask = (delay > lower) & (delay <= float(bucket))
        result[f"{int(lower)}_{bucket}ms"] = float(quality[mask].mean()) if mask.any() else 0.0
        lower = float(bucket)
    mask = delay > lower
    result[f"gt_{int(lower)}ms"] = float(quality[mask].mean()) if mask.any() else 0.0
    return result
