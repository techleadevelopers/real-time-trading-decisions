from __future__ import annotations

import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict


def default_rtts_weights() -> Dict[str, Any]:
    return {
        "version": 1,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "source": "python-export-default",
        "reversal_classifier": {
            "movement_drop_pct": 0.45,
            "movement_velocity": 0.35,
            "movement_volume_burst": 0.20,
            "intent_exhaustion": 0.25,
            "intent_weak_signal": 0.20,
            "intent_absorption": 0.20,
            "intent_timing": 0.20,
            "intent_liquidity_ok": 0.15,
            "trigger_observation_bonus": 0.15,
            "continuation_directionality": 0.35,
            "continuation_momentum": 0.25,
            "continuation_imbalance": 0.20,
            "continuation_aggression": 0.20,
            "ready_bias_threshold": 0.55,
        },
        "entry_scoring": {
            "reversal_probability": 0.40,
            "intent_score": 0.20,
            "trigger_edge": 0.15,
            "reversal_edge": 0.10,
            "edge_reliability": 0.15,
            "min_enter_score": 0.58,
            "min_enter_confidence": 0.52,
            "min_wait_probability": 0.45,
        },
    }


def export_rtts_weights(output_path: str, overrides: Dict[str, Any] | None = None) -> str:
    payload = default_rtts_weights()
    if overrides:
        deep_merge(payload, overrides)

    target = Path(output_path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    return str(target)


def deep_merge(base: Dict[str, Any], updates: Dict[str, Any]) -> Dict[str, Any]:
    for key, value in updates.items():
        if isinstance(value, dict) and isinstance(base.get(key), dict):
            deep_merge(base[key], value)
        else:
            base[key] = value
    return base


if __name__ == "__main__":
    path = export_rtts_weights("data/models/rtts_reversal_weights.json")
    print(path)
