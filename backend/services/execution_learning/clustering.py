from __future__ import annotations

from dataclasses import dataclass
from typing import Dict

import numpy as np
import pandas as pd
from sklearn.cluster import KMeans
from sklearn.metrics import silhouette_score
from sklearn.pipeline import Pipeline
from sklearn.preprocessing import RobustScaler

from .features import safe_col
from .postmortem import compute_execution_postmortem


CLUSTER_FEATURES = [
    "execution_quality_score",
    "fill_efficiency_score",
    "slippage_error",
    "latency_penalty",
    "adverse_selection_score",
    "competition_density_index",
    "market_microstructure_stress",
    "execution_fragility_index",
]


@dataclass
class StrategyClusteringModel:
    model: object | None = None
    cluster_quality_scores_: Dict[int, float] | None = None
    cluster_names_: Dict[int, str] | None = None
    metrics_: Dict[str, float | bool | str] | None = None

    def fit(self, raw: pd.DataFrame, n_clusters: int = 3) -> "StrategyClusteringModel":
        scored = compute_execution_postmortem(raw)
        if len(scored) < n_clusters * 10:
            self.model = None
            self.cluster_quality_scores_ = {}
            self.cluster_names_ = {}
            self.metrics_ = {"trained": False, "reason": "insufficient_cluster_samples"}
            return self

        x = scored[CLUSTER_FEATURES].astype(float).values
        self.model = Pipeline(
            [
                ("scaler", RobustScaler()),
                ("kmeans", KMeans(n_clusters=n_clusters, n_init=20, random_state=42)),
            ]
        )
        labels = self.model.fit_predict(x)
        scored = scored.copy()
        scored["strategy_cluster_id"] = labels

        quality: Dict[int, float] = {}
        for cluster_id in sorted(set(labels)):
            subset = scored[scored["strategy_cluster_id"] == cluster_id]
            q = float(
                0.45 * safe_col(subset, "execution_quality_score").mean()
                + 0.25 * safe_col(subset, "fill_efficiency_score").mean()
                + 0.15 * (1.0 - safe_col(subset, "adverse_selection_score").mean())
                + 0.15 * (1.0 - safe_col(subset, "competition_density_index").mean())
            )
            quality[int(cluster_id)] = float(np.clip(q, 0.0, 1.0))

        ordered = sorted(quality.items(), key=lambda item: item[1])
        names = {
            ordered[0][0]: "HOSTILE_EXECUTION_REGIME",
            ordered[-1][0]: "HIGH_QUALITY_EXECUTION_REGIME",
        }
        for cluster_id, _ in ordered[1:-1]:
            names[cluster_id] = "NEUTRAL_EXECUTION_REGIME"

        self.cluster_quality_scores_ = quality
        self.cluster_names_ = names
        metrics: Dict[str, float | bool | str] = {"trained": True}
        if len(set(labels)) > 1:
            try:
                metrics["silhouette_score"] = float(silhouette_score(x, labels))
            except ValueError:
                metrics["silhouette_score"] = 0.0
        self.metrics_ = metrics
        return self

    def predict(self, raw: pd.DataFrame) -> pd.DataFrame:
        scored = compute_execution_postmortem(raw)
        if self.model is None:
            result = pd.DataFrame(index=scored.index)
            result["strategy_cluster_id"] = -1
            result["strategy_cluster"] = "NEUTRAL_EXECUTION_REGIME"
            result["cluster_quality_score"] = 0.5
            return result

        labels = self.model.predict(scored[CLUSTER_FEATURES].astype(float).values)
        names = self.cluster_names_ or {}
        quality = self.cluster_quality_scores_ or {}
        result = pd.DataFrame(index=scored.index)
        result["strategy_cluster_id"] = labels
        result["strategy_cluster"] = [names.get(int(label), "NEUTRAL_EXECUTION_REGIME") for label in labels]
        result["cluster_quality_score"] = [quality.get(int(label), 0.5) for label in labels]
        return result
