from __future__ import annotations

import os
from typing import Any, Dict, List

import pandas as pd
from fastapi import APIRouter
from pydantic import BaseModel, Field

from services.execution_learning.pipeline import ExecutionLearningLab


MODEL_PATH = os.getenv(
    "EXECUTION_LEARNING_MODEL_PATH",
    "data/models/execution_learning_lab.pkl",
)

router = APIRouter()


class ExecutionLogBatch(BaseModel):
    records: List[Dict[str, Any]] = Field(default_factory=list)


class AnalyzeRequest(ExecutionLogBatch):
    train_inline_if_missing: bool = False


def _frame(records: List[Dict[str, Any]]) -> pd.DataFrame:
    if not records:
        raise ValueError("records is empty")
    return pd.DataFrame.from_records(records)


@router.post("/train")
def train(batch: ExecutionLogBatch):
    try:
        df = _frame(batch.records)
        lab = ExecutionLearningLab.train(df)
        os.makedirs(os.path.dirname(MODEL_PATH), exist_ok=True)
        lab.save(MODEL_PATH)
        return {
            "ok": True,
            "model_path": MODEL_PATH,
            "metrics": lab.metrics,
            "safety": {
                "outputs_live_orders": False,
                "outputs_trade_direction": False,
                "objective": "execution_quality_only",
            },
        }
    except Exception as exc:
        return {"ok": False, "error": str(exc)}


@router.post("/analyze")
def analyze(req: AnalyzeRequest):
    try:
        df = _frame(req.records)
        if os.path.exists(MODEL_PATH):
            lab = ExecutionLearningLab.load(MODEL_PATH)
        elif req.train_inline_if_missing:
            lab = ExecutionLearningLab.train(df)
        else:
            return {"ok": False, "error": "execution learning model not found"}
        return {"ok": True, **lab.analyze(df)}
    except Exception as exc:
        return {"ok": False, "error": str(exc)}


@router.post("/policy")
def policy(req: AnalyzeRequest):
    try:
        df = _frame(req.records)
        if os.path.exists(MODEL_PATH):
            lab = ExecutionLearningLab.load(MODEL_PATH)
        elif req.train_inline_if_missing:
            lab = ExecutionLearningLab.train(df)
        else:
            return {"ok": False, "error": "execution learning model not found"}
        return {"ok": True, "policy_update": lab.policy_update(df)}
    except Exception as exc:
        return {"ok": False, "error": str(exc)}


@router.get("/meta")
def meta():
    exists = os.path.exists(MODEL_PATH)
    return {
        "ok": True,
        "model_path": MODEL_PATH,
        "model_exists": exists,
        "python_can_place_orders": False,
        "python_can_emit_trade_direction": False,
        "objective": "execution_quality_only",
        "truth_only": True,
    }
