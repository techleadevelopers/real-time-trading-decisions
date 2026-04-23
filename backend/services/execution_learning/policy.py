from __future__ import annotations

from datetime import datetime, timezone
from typing import Any, Dict

import numpy as np
import pandas as pd

from .features import generate_execution_features, safe_col
from .schemas import ExecutionMode, ExecutionPolicyUpdate, PolicyBlockCondition


def _mode_from_aggression(aggression: float) -> ExecutionMode:
    if aggression <= 0.15:
        return ExecutionMode.DEFENSIVE
    if aggression <= 0.40:
        return ExecutionMode.PASSIVE
    if aggression >= 0.70:
        return ExecutionMode.AGGRESSIVE
    return ExecutionMode.NEUTRAL


def _training_window(raw: pd.DataFrame) -> Dict[str, str]:
    now = datetime.now(timezone.utc).isoformat()
    if raw.empty or "timestamp" not in raw:
        return {"start": now, "end": now}
    ts = pd.to_datetime(raw["timestamp"], errors="coerce", utc=True).dropna()
    if ts.empty:
        return {"start": now, "end": now}
    return {"start": ts.min().isoformat(), "end": ts.max().isoformat()}


def build_policy_update(
    raw: pd.DataFrame,
    survival_probability: np.ndarray | None = None,
    cluster_summary: Dict[str, Any] | None = None,
    model_metrics: Dict[str, Any] | None = None,
) -> ExecutionPolicyUpdate:
    df = generate_execution_features(raw)
    if survival_probability is None or len(survival_probability) != len(df):
        survival_probability = np.asarray(
            1.0
            - (
                0.35 * safe_col(df, "competition_density_index")
                + 0.25 * safe_col(df, "execution_fragility_index")
                + 0.25 * safe_col(df, "market_microstructure_stress")
                + 0.15 * safe_col(df, "latency_penalty")
            ),
            dtype=float,
        )
    survival = float(np.clip(np.nanmean(survival_probability) if len(survival_probability) else 0.0, 0.0, 1.0))
    stress = float(safe_col(df, "market_microstructure_stress").mean()) if not df.empty else 1.0
    competition = float(safe_col(df, "competition_density_index").mean()) if not df.empty else 1.0
    fragility = float(safe_col(df, "execution_fragility_index").mean()) if not df.empty else 1.0
    latency = float(safe_col(df, "latency_penalty").mean()) if not df.empty else 1.0

    risk_base = float(np.clip(0.25 + 0.75 * survival - 0.30 * stress - 0.25 * competition - 0.20 * fragility, 0.0, 1.15))
    aggression = float(np.clip(0.20 + 0.75 * survival - 0.40 * competition - 0.25 * latency, 0.0, 1.0))

    risk_by_regime = {
        "NORMAL": risk_base,
        "HIGH_VOLATILITY": float(np.clip(risk_base * 0.65, 0.0, 1.0)),
        "NEWS_SHOCK": 0.0,
        "LOW_LIQUIDITY": float(np.clip(risk_base * 0.35, 0.0, 0.60)),
        "TREND_EXPANSION": float(np.clip(risk_base * 1.10, 0.0, 1.20)),
        "HOSTILE_EXECUTION": 0.0,
    }

    symbol_adjustments: Dict[str, Dict[str, Any]] = {}
    if "symbol" in df:
        for symbol, subset in df.groupby(df["symbol"].astype(str).str.upper()):
            if not symbol:
                continue
            symbol_survival = float(np.clip(1.0 - safe_col(subset, "market_microstructure_stress").mean(), 0.0, 1.0))
            symbol_competition = float(safe_col(subset, "competition_density_index").mean())
            symbol_aggression = float(np.clip(0.25 + 0.70 * symbol_survival - 0.35 * symbol_competition, 0.0, 1.0))
            symbol_adjustments[symbol] = {
                "risk_multiplier": float(np.clip(risk_base * (0.5 + 0.7 * symbol_survival), 0.0, 1.2)),
                "max_size_adjustment_factor": float(np.clip(0.25 + 0.75 * symbol_survival, 0.0, 1.0)),
                "aggression_level": symbol_aggression,
                "preferred_execution_mode": _mode_from_aggression(symbol_aggression).value,
                "min_survival_probability": float(np.clip(0.55 + 0.20 * symbol_competition, 0.55, 0.80)),
            }

    cluster_policy = cluster_summary or {
        "HIGH_QUALITY_EXECUTION_REGIME": {
            "risk_multiplier": 1.05,
            "aggression_level": min(0.75, aggression + 0.10),
            "preferred_execution_mode": "neutral",
        },
        "NEUTRAL_EXECUTION_REGIME": {
            "risk_multiplier": min(0.80, risk_base),
            "aggression_level": min(0.50, aggression),
            "preferred_execution_mode": "passive",
        },
        "HOSTILE_EXECUTION_REGIME": {
            "risk_multiplier": 0.0,
            "aggression_level": 0.0,
            "preferred_execution_mode": "defensive",
        },
    }

    return ExecutionPolicyUpdate(
        policy_version="execution-learning-v1",
        generated_at=datetime.now(timezone.utc).isoformat(),
        training_window=_training_window(raw),
        risk_multiplier_by_regime=risk_by_regime,
        max_size_adjustment_factor=float(np.clip(risk_base, 0.0, 1.0)),
        aggression_level=aggression,
        preferred_execution_mode=_mode_from_aggression(aggression),
        block_conditions=[
            PolicyBlockCondition("competition_too_high", "competition_density_index >= 0.85"),
            PolicyBlockCondition("inclusion_collapse", "inclusion_drift_score >= 0.75"),
            PolicyBlockCondition("latency_edge_destroyed", "latency_penalty >= 0.80"),
            PolicyBlockCondition("liquidity_collapse", "orderbook_depth_shock >= 0.80"),
            PolicyBlockCondition("adverse_selection_dominant", "adverse_selection_probability >= 0.65"),
            PolicyBlockCondition("negative_execution_survival", "survival_probability < min_survival_probability"),
        ],
        symbol_adjustments=symbol_adjustments,
        cluster_policy=cluster_policy,
        model_metrics=model_metrics or {},
    )
