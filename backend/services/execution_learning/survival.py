from __future__ import annotations

from dataclasses import dataclass
from typing import Dict

import numpy as np
import pandas as pd
from sklearn.calibration import CalibratedClassifierCV
from sklearn.ensemble import HistGradientBoostingClassifier
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import brier_score_loss, roc_auc_score
from sklearn.pipeline import Pipeline
from sklearn.preprocessing import StandardScaler

from .features import generate_execution_features, safe_col


SURVIVAL_FEATURES = [
    "orderbook_imbalance_multilevel",
    "trade_velocity",
    "volatility_micro_spike",
    "snapshot_staleness_score",
    "fill_probability_estimate",
    "latency_penalty",
    "competition_density_index",
    "execution_fragility_index",
    "inclusion_drift_score",
    "market_microstructure_stress",
    "regime_instability_score",
    "orderbook_depth_shock",
    "spread_change_rate",
    "mempool_pressure",
    "volatility_regime_score",
]


def build_survival_target(raw: pd.DataFrame) -> pd.Series:
    filled = safe_col(raw, "filled", 0.0) > 0.5
    if "filled" not in raw:
        filled = safe_col(raw, "filled_size", 0.0) > 0.0
    fees = safe_col(raw, "fees", 0.0)
    markout = (
        0.35 * safe_col(raw, "markout_100ms")
        + 0.30 * safe_col(raw, "markout_500ms")
        + 0.20 * safe_col(raw, "markout_1s")
        + 0.15 * safe_col(raw, "markout_5s")
    )
    adverse = safe_col(raw, "adverse_selection", 0.0) > 0.5
    return (filled & ((markout - fees) >= 0.0) & ~adverse).astype(int)


@dataclass
class ExecutionSurvivalModel:
    model: object | None = None
    metrics_: Dict[str, float | bool | str] | None = None

    def fit(self, raw: pd.DataFrame) -> "ExecutionSurvivalModel":
        df = generate_execution_features(raw)
        y = raw["survived"].astype(int) if "survived" in raw else build_survival_target(raw)
        x = df[SURVIVAL_FEATURES].astype(float).values
        if len(df) < 40 or y.nunique() < 2:
            self.model = None
            self.metrics_ = {"trained": False, "reason": "insufficient_survival_variation"}
            return self

        split = max(1, int(len(df) * 0.8))
        x_train, x_val = x[:split], x[split:]
        y_train, y_val = y.iloc[:split], y.iloc[split:]

        if len(df) >= 250:
            base = HistGradientBoostingClassifier(max_leaf_nodes=15, learning_rate=0.05, max_iter=140)
        else:
            base = Pipeline(
                [
                    ("scaler", StandardScaler()),
                    ("lr", LogisticRegression(max_iter=500, class_weight="balanced")),
                ]
            )
        base.fit(x_train, y_train)
        if len(x_val) > 0 and y_val.nunique() > 1:
            try:
                self.model = CalibratedClassifierCV(base, cv="prefit", method="sigmoid")
                self.model.fit(x_val, y_val)
            except Exception:
                self.model = base
        else:
            self.model = base

        metrics: Dict[str, float | bool | str] = {"trained": True}
        if len(x_val) > 0 and y_val.nunique() > 1:
            p = self.predict_proba(df.iloc[split:])
            metrics["brier_score"] = float(brier_score_loss(y_val, p))
            try:
                metrics["roc_auc"] = float(roc_auc_score(y_val, p))
            except ValueError:
                metrics["roc_auc"] = 0.0
        self.metrics_ = metrics
        return self

    def predict_proba(self, rows: pd.DataFrame) -> np.ndarray:
        df = generate_execution_features(rows)
        if self.model is None:
            base = 1.0 - (
                0.30 * safe_col(df, "latency_penalty")
                + 0.25 * safe_col(df, "competition_density_index")
                + 0.25 * safe_col(df, "market_microstructure_stress")
                + 0.10 * safe_col(df, "execution_fragility_index")
                + 0.10 * (1.0 - safe_col(df, "fill_probability_estimate"))
            )
            return np.asarray(np.clip(base, 0.0, 1.0), dtype=float)
        x = df[SURVIVAL_FEATURES].astype(float).values
        return self.model.predict_proba(x)[:, 1]
