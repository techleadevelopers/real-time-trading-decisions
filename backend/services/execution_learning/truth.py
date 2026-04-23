from __future__ import annotations

import pandas as pd


TRUTH_REQUIRED_COLUMNS = [
    "filled_size",
    "realized_slippage_bps",
    "fees",
    "markout_100ms",
    "markout_500ms",
    "markout_1s",
    "markout_5s",
]


def validate_truth_only(raw: pd.DataFrame) -> None:
    if raw.empty:
        raise ValueError("execution learning requires non-empty real execution logs")
    missing = [name for name in TRUTH_REQUIRED_COLUMNS if name not in raw.columns]
    if missing:
        raise ValueError(f"missing execution-truth columns: {missing}")
    if "simulated" in raw.columns and raw["simulated"].astype(bool).any():
        raise ValueError("simulated fills are not allowed in execution learning")
    if "is_paper" in raw.columns and raw["is_paper"].astype(bool).any():
        raise ValueError("paper fills are not allowed in execution learning")
    if "source" in raw.columns:
        blocked = raw["source"].astype(str).str.lower().isin({"sim", "simulation", "paper", "backtest"})
        if blocked.any():
            raise ValueError("simulation/backtest sources are not allowed in execution learning")
