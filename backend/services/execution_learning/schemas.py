from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Dict, List


class ExecutionFailureLabel(str, Enum):
    OUTBID = "OUTBID"
    LATENCY_FAILURE = "LATENCY_FAILURE"
    SLIPPAGE_EXPANSION = "SLIPPAGE_EXPANSION"
    ADVERSE_SELECTION = "ADVERSE_SELECTION"
    LIQUIDITY_COLLAPSE = "LIQUIDITY_COLLAPSE"
    GOOD_EXECUTION = "GOOD_EXECUTION"


class ExecutionMode(str, Enum):
    PASSIVE = "passive"
    NEUTRAL = "neutral"
    AGGRESSIVE = "aggressive"
    DEFENSIVE = "defensive"


@dataclass(frozen=True)
class PolicyBlockCondition:
    name: str
    condition: str


@dataclass
class ExecutionPolicyUpdate:
    policy_version: str
    generated_at: str
    training_window: Dict[str, str]
    risk_multiplier_by_regime: Dict[str, float]
    max_size_adjustment_factor: float
    aggression_level: float
    preferred_execution_mode: ExecutionMode
    block_conditions: List[PolicyBlockCondition]
    symbol_adjustments: Dict[str, Dict[str, Any]] = field(default_factory=dict)
    cluster_policy: Dict[str, Dict[str, Any]] = field(default_factory=dict)
    model_metrics: Dict[str, Any] = field(default_factory=dict)
    safety: Dict[str, bool] = field(
        default_factory=lambda: {
            "python_can_place_orders": False,
            "python_can_emit_trade_direction": False,
            "requires_go_risk_gate": True,
            "requires_rust_final_execution_gate": True,
        }
    )

    def to_dict(self) -> Dict[str, Any]:
        return {
            "policy_version": self.policy_version,
            "generated_at": self.generated_at,
            "training_window": self.training_window,
            "global_policy": {
                "risk_multiplier_by_regime": self.risk_multiplier_by_regime,
                "max_size_adjustment_factor": self.max_size_adjustment_factor,
                "aggression_level": self.aggression_level,
                "preferred_execution_mode": self.preferred_execution_mode.value,
            },
            "block_conditions": [
                {"name": item.name, "condition": item.condition}
                for item in self.block_conditions
            ],
            "symbol_adjustments": self.symbol_adjustments,
            "cluster_policy": self.cluster_policy,
            "model_metrics": self.model_metrics,
            "safety": self.safety,
        }
