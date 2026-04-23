from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict

import joblib
import pandas as pd

from .clustering import StrategyClusteringModel
from .failure_classifier import FailureClassifier
from .features import FEATURE_COLUMNS, generate_execution_features
from .policy import build_policy_update
from .postmortem import compute_execution_postmortem, latency_penalty_curve, markout_curve_by_window
from .regime import adversarial_regime_scores
from .return_model import ExecutionAdjustedReturnModel
from .survival import ExecutionSurvivalModel
from .truth import validate_truth_only


@dataclass
class ExecutionLearningLab:
    failure_classifier: FailureClassifier
    survival_model: ExecutionSurvivalModel
    return_model: ExecutionAdjustedReturnModel
    clustering_model: StrategyClusteringModel
    metrics: Dict[str, Any]

    @classmethod
    def train(cls, raw: pd.DataFrame) -> "ExecutionLearningLab":
        validate_truth_only(raw)
        postmortem = compute_execution_postmortem(raw)
        failure = FailureClassifier().fit(postmortem)
        survival = ExecutionSurvivalModel().fit(postmortem)
        returns = ExecutionAdjustedReturnModel().fit(postmortem)
        clustering = StrategyClusteringModel().fit(postmortem)
        metrics = {
            "samples": int(len(postmortem)),
            "features": FEATURE_COLUMNS,
            "postmortem": {
                "execution_quality_score_mean": float(postmortem["execution_quality_score"].mean()) if not postmortem.empty else 0.0,
                "latency_penalty_curve": latency_penalty_curve(postmortem),
                "markout_curve_by_time_window": markout_curve_by_window(postmortem),
            },
            "failure_classifier": failure.metrics_ or {},
            "survival_model": survival.metrics_ or {},
            "execution_adjusted_return_model": returns.metrics_ or {},
            "strategy_clustering": clustering.metrics_ or {},
            "truth_only": True,
        }
        return cls(failure, survival, returns, clustering, metrics)

    def analyze(self, raw: pd.DataFrame) -> Dict[str, Any]:
        validate_truth_only(raw)
        features = generate_execution_features(raw)
        postmortem = compute_execution_postmortem(features)
        failure_proba = self.failure_classifier.predict_proba(postmortem)
        survival = self.survival_model.predict_proba(postmortem)
        adjusted_return = self.return_model.predict(postmortem)
        adversarial_regimes = adversarial_regime_scores(postmortem)
        clusters = self.clustering_model.predict(postmortem)
        policy = build_policy_update(
            postmortem,
            survival_probability=survival,
            model_metrics=self.metrics,
        )
        return {
            "features": features.to_dict(orient="records"),
            "postmortem": postmortem.to_dict(orient="records"),
            "failure_probability_vector": failure_proba.to_dict(orient="records"),
            "survival_probability": survival.tolist(),
            "execution_adjusted_return": adjusted_return.tolist(),
            "adversarial_regimes": adversarial_regimes.to_dict(orient="records"),
            "strategy_clusters": clusters.to_dict(orient="records"),
            "policy_update": policy.to_dict(),
            "safety": {
                "outputs_live_orders": False,
                "outputs_trade_direction": False,
                "objective": "execution_quality_only",
                "truth_only": True,
            },
        }

    def policy_update(self, raw: pd.DataFrame) -> Dict[str, Any]:
        validate_truth_only(raw)
        postmortem = compute_execution_postmortem(raw)
        survival = self.survival_model.predict_proba(postmortem)
        return build_policy_update(
            postmortem,
            survival_probability=survival,
            model_metrics=self.metrics,
        ).to_dict()

    def save(self, path: str) -> str:
        joblib.dump(self, path)
        return path

    @staticmethod
    def load(path: str) -> "ExecutionLearningLab":
        return joblib.load(path)


def train_execution_learning_lab(raw: pd.DataFrame, model_path: str | None = None) -> ExecutionLearningLab:
    lab = ExecutionLearningLab.train(raw)
    if model_path:
        lab.save(model_path)
    return lab
