import {
  DEMO_SIGNALS, DEMO_ALERTS, DEMO_METRICS, DEMO_RISK,
  DEMO_ACCOUNT_BASE, DEMO_POSITIONS, DEMO_LEDGER, DEMO_EVENTS,
  DEMO_RECON, livePrice,
} from './demo';

// Simulate slight variations on each call for live feel
function varSignals() {
  return DEMO_SIGNALS.map((s) => ({
    ...s,
    entry_price: s.entry_price ? livePrice(s.entry_price, 0.0008) : null,
    score: Math.max(0, Math.min(100, s.score + Math.round((Math.random() - 0.5) * 3))),
    probability: Math.max(0.01, Math.min(0.99, s.probability + (Math.random() - 0.5) * 0.02)),
    timestamp: new Date().toISOString(),
  }));
}

function delay(ms = 400) {
  return new Promise((r) => setTimeout(r, ms + Math.random() * 200));
}

export async function fetchHealth() {
  await delay(80);
  return true;
}

export async function fetchSignals(minScore = 0, onlyStrong = false) {
  await delay(500);
  let list = varSignals();
  if (minScore > 0) list = list.filter((s) => s.score >= minScore);
  if (onlyStrong) list = list.filter((s) => s.signal.includes('FORTE'));
  return list.sort((a, b) => b.score - a.score);
}

export async function fetchSignalsLatest() {
  await delay(100);
  return { lastUpdate: new Date().toISOString() };
}

export async function fetchAlerts() {
  await delay(250);
  return DEMO_ALERTS;
}

export async function fetchMetrics() {
  await delay(300);
  return DEMO_METRICS;
}

export async function fetchCPHealth() {
  await delay(80);
  return true;
}

export async function fetchCPStatus() {
  await delay(200);
  return { ok: true };
}

export async function fetchCPRisk() {
  await delay(300);
  return {
    ...DEMO_RISK,
    SystemStressIndex: Math.max(0, DEMO_RISK.SystemStressIndex + (Math.random() - 0.5) * 0.04),
    MempoolPressure: Math.max(0, DEMO_RISK.MempoolPressure + (Math.random() - 0.5) * 0.02),
    ExecutionFragility: Math.max(0, DEMO_RISK.ExecutionFragility + (Math.random() - 0.5) * 0.03),
  };
}

export async function fetchCPPositions() {
  await delay(300);
  return DEMO_POSITIONS.map((p) => ({
    ...p,
    AvgPrice: livePrice(p.AvgPrice, 0.0005),
  }));
}

export async function fetchCPAccount() {
  await delay(350);
  return {
    ...DEMO_ACCOUNT_BASE,
    UnrealizedPnl: DEMO_ACCOUNT_BASE.UnrealizedPnl + (Math.random() - 0.5) * 40,
    Available: DEMO_ACCOUNT_BASE.Available + (Math.random() - 0.5) * 20,
  };
}

export async function fetchCPLedger() {
  await delay(400);
  return DEMO_LEDGER;
}

export async function fetchCPEvents() {
  await delay(350);
  return DEMO_EVENTS;
}

export async function fetchCPReconciliation() {
  await delay(300);
  return DEMO_RECON;
}

export async function postKillSwitch(_enabled: boolean) {
  await delay(300);
  return { ok: true };
}
