from __future__ import annotations

from dataclasses import dataclass
from typing import Dict

import numpy as np
import pandas as pd
from sklearn.ensemble import HistGradientBoostingRegressor
from sklearn.linear_model import HuberRegressor
from sklearn.metrics import mean_absolute_error, r2_score
from sklearn.pipeline import Pipeline
from sklearn.preprocessing import RobustScaler

from .features import generate_execution_features, safe_col


RETURN_FEATURES = [
    "orderbook_imbalance_multilevel",
    "spread_change_rate",
    "trade_velocity",
    "volatility_micro_spike",
    "latency_penalty",
    "snapshot_staleness_score",
    "competition_density_index",
    "fill_probability_estimate",
    "execution_fragility_index",
    "market_microstructure_stress",
]


def execution_adjusted_return_target(raw: pd.DataFrame) -> pd.Series:
    realized_pnl = safe_col(raw, "realized_pnl")
    fees = safe_col(raw, "fees")
    slippage_cost = safe_col(raw, "slippage_cost")
    if "slippage_cost" not in raw:
        notional = safe_col(raw, "filled_notional")
        if "filled_notional" not in raw:
            notional = safe_col(raw, "filled_size") * safe_col(raw, "fill_price")
        slippage_cost = notional * safe_col(raw, "realized_slippage_bps").abs() / 10_000.0
    return (realized_pnl - fees - slippage_cost).astype(float)


@dataclass
class ExecutionAdjustedReturnModel:
    model: object | None = None
    metrics_: Dict[str, float | bool | str] | None = None

    def fit(self, raw: pd.DataFrame) -> "ExecutionAdjustedReturnModel":
        df = generate_execution_features(raw)
        y = execution_adjusted_return_target(raw)
        x = df[RETURN_FEATURES].astype(float).values
        if len(df) < 40 or float(y.abs().sum()) <= 1e-12:
            self.model = None
            self.metrics_ = {"trained": False, "reason": "insufficient_realized_return_variation"}
            return self

        split = max(1, int(len(df) * 0.8))
        x_train, x_val = x[:split], x[split:]
        y_train, y_val = y.iloc[:split], y.iloc[split:]
        if len(df) >= 250:
            self.model = HistGradientBoostingRegressor(max_leaf_nodes=15, learning_rate=0.05, max_iter=160)
        else:
            self.model = Pipeline([("scaler", RobustScaler()), ("huber", HuberRegressor())])
        self.model.fit(x_train, y_train)

        metrics: Dict[str, float | bool | str] = {"trained": True}
        if len(x_val) > 0:
            pred = self.model.predict(x_val)
            metrics["mae_execution_adjusted_return"] = float(mean_absolute_error(y_val, pred))
            try:
                metrics["r2_execution_adjusted_return"] = float(r2_score(y_val, pred))
            except ValueError:
                metrics["r2_execution_adjusted_return"] = 0.0
        self.metrics_ = metrics
        return self

    def predict(self, rows: pd.DataFrame) -> np.ndarray:
        df = generate_execution_features(rows)
        if self.model is None:
            markout = (
                0.35 * safe_col(df, "markout_100ms")
                + 0.30 * safe_col(df, "markout_500ms")
                + 0.20 * safe_col(df, "markout_1s")
                + 0.15 * safe_col(df, "markout_5s")
            )
            fees = safe_col(df, "fees")
            slip = safe_col(df, "slippage_cost")
            return np.asarray(markout - fees - slip, dtype=float)
        return self.model.predict(df[RETURN_FEATURES].astype(float).values)
