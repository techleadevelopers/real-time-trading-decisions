from __future__ import annotations

import numpy as np
import pandas as pd

from .features import generate_execution_features, safe_col


def adversarial_regime_scores(raw: pd.DataFrame) -> pd.DataFrame:
    df = generate_execution_features(raw)
    out = pd.DataFrame(index=df.index)
    out["high_competition"] = safe_col(df, "competition_density_index")
    out["toxic_liquidity"] = np.maximum(
        safe_col(df, "adverse_selection_score"),
        0.5 * safe_col(df, "orderbook_depth_shock") + 0.5 * safe_col(df, "spread_change_rate"),
    )
    out["latency_sensitive"] = np.maximum(
        safe_col(df, "latency_penalty"),
        safe_col(df, "snapshot_staleness_score"),
    )
    out["low_survival_setup"] = np.maximum(
        safe_col(df, "execution_fragility_index"),
        safe_col(df, "market_microstructure_stress"),
    )
    out["adversarial_pressure"] = (
        0.30 * out["high_competition"]
        + 0.30 * out["toxic_liquidity"]
        + 0.20 * out["latency_sensitive"]
        + 0.20 * out["low_survival_setup"]
    ).clip(0.0, 1.0)
    labels = np.full(len(out), "NORMAL_EXECUTION", dtype=object)
    labels[out["low_survival_setup"] >= 0.70] = "LOW_SURVIVAL_PROBABILITY"
    labels[out["latency_sensitive"] >= 0.70] = "LATENCY_SENSITIVE"
    labels[out["toxic_liquidity"] >= 0.70] = "TOXIC_LIQUIDITY"
    labels[out["high_competition"] >= 0.70] = "HIGH_COMPETITION"
    out["adversarial_regime"] = labels
    return out
