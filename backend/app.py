import logging
from fastapi import FastAPI, Depends
from fastapi.middleware.cors import CORSMiddleware
from routers import health, data, model, backtest
from routers import frontend
from routers import regime
from routers import execution_learning
from services.market_stream import start_stream
from services.utils import API_ALLOW_ORIGINS
from deps import verify_token

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s %(name)s - %(message)s",
)

app = FastAPI(title="Signals API", version="0.1.0")

# CORS para frontend (localhost:3000)
app.add_middleware(
    CORSMiddleware,
    allow_origins=API_ALLOW_ORIGINS,
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

app.include_router(health.router, prefix="/health", tags=["health"])
app.include_router(data.router, prefix="/data", tags=["data"])
app.include_router(
    model.router,
    prefix="/model",
    tags=["model"],
    dependencies=[Depends(verify_token)],
)
app.include_router(backtest.router, prefix="/backtest", tags=["backtest"])
app.include_router(frontend.router, prefix="/api", tags=["frontend"])
app.include_router(
    regime.router,
    prefix="/regime",
    tags=["regime"],
    dependencies=[Depends(verify_token)],
)
app.include_router(
    execution_learning.router,
    prefix="/execution-learning",
    tags=["execution-learning"],
    dependencies=[Depends(verify_token)],
)


@app.on_event("startup")
def _start_ws_stream():
    start_stream()
