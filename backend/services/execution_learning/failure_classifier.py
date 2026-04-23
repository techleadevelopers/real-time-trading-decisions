from __future__ import annotations

from dataclasses import dataclass
from typing import Dict

import numpy as np
import pandas as pd
from sklearn.calibration import CalibratedClassifierCV
from sklearn.ensemble import HistGradientBoostingClassifier
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import classification_report
from sklearn.pipeline import Pipeline
from sklearn.preprocessing import StandardScaler

from .features import generate_execution_features, safe_col
from .schemas import ExecutionFailureLabel


FAILURE_FEATURES = [
    "mempool_pressure",
    "competition_density_index",
    "spread_change_rate",
    "orderbook_depth_shock",
    "execution_delay_ms",
    "volatility_regime_score",
    "execution_fragility_index",
    "inclusion_drift_score",
    "market_microstructure_stress",
]


def infer_failure_labels(raw: pd.DataFrame) -> pd.Series:
    df = generate_execution_features(raw)
    outbid = safe_col(df, "outbid", 0.0) > 0.5
    filled = safe_col(df, "filled", 0.0) > 0.5
    rejected = safe_col(df, "rejected", 0.0) > 0.5
    delay = safe_col(df, "execution_delay_ms")
    max_delay = safe_col(df, "latency_failure_threshold_ms", 150.0)
    slip_error = safe_col(df, "slippage_error")
    adverse = safe_col(df, "adverse_selection_score")
    liquidity = safe_col(df, "orderbook_depth_shock")

    labels = pd.Series(ExecutionFailureLabel.GOOD_EXECUTION.value, index=df.index, dtype=object)
    labels.loc[(liquidity >= 0.75) & (rejected | ~filled)] = ExecutionFailureLabel.LIQUIDITY_COLLAPSE.value
    labels.loc[(slip_error >= 8.0) & filled] = ExecutionFailureLabel.SLIPPAGE_EXPANSION.value
    labels.loc[(delay >= max_delay) & ~outbid] = ExecutionFailureLabel.LATENCY_FAILURE.value
    labels.loc[outbid] = ExecutionFailureLabel.OUTBID.value
    labels.loc[(adverse >= 0.60) & filled] = ExecutionFailureLabel.ADVERSE_SELECTION.value
    return labels


@dataclass
class FailureClassifier:
    model: object | None = None
    labels_: list[str] | None = None
    metrics_: Dict[str, object] | None = None

    def fit(self, raw: pd.DataFrame) -> "FailureClassifier":
        df = generate_execution_features(raw)
        y = raw["failure_label"].astype(str) if "failure_label" in raw else infer_failure_labels(raw)
        x = df[FAILURE_FEATURES].astype(float).values
        if len(df) < 40 or y.nunique() < 2:
            self.model = None
            self.labels_ = sorted(y.unique().tolist())
            self.metrics_ = {"trained": False, "reason": "insufficient_labeled_failure_variation"}
            return self

        split = max(1, int(len(df) * 0.8))
        x_train, x_val = x[:split], x[split:]
        y_train, y_val = y.iloc[:split], y.iloc[split:]

        if len(df) >= 250:
            base = HistGradientBoostingClassifier(max_leaf_nodes=15, learning_rate=0.06, max_iter=120)
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

        self.labels_ = list(getattr(self.model, "classes_", sorted(y.unique().tolist())))
        if len(x_val) > 0:
            pred = self.model.predict(x_val)
            self.metrics_ = {
                "trained": True,
                "classification_report": classification_report(y_val, pred, zero_division=0, output_dict=True),
            }
        else:
            self.metrics_ = {"trained": True}
        return self

    def predict_proba(self, rows: pd.DataFrame) -> pd.DataFrame:
        df = generate_execution_features(rows)
        labels = [label.value for label in ExecutionFailureLabel]
        if self.model is None:
            result = pd.DataFrame(0.0, index=df.index, columns=labels)
            result[ExecutionFailureLabel.GOOD_EXECUTION.value] = 1.0
            return result

        x = df[FAILURE_FEATURES].astype(float).values
        proba = self.model.predict_proba(x)
        result = pd.DataFrame(0.0, index=df.index, columns=labels)
        for idx, label in enumerate(getattr(self.model, "classes_", [])):
            result[str(label)] = proba[:, idx]
        row_sum = result.sum(axis=1).replace(0.0, np.nan)
        return result.div(row_sum, axis=0).fillna(0.0)
