// ── Demo data simulating live scalp bot on BingX Futures ──

export const DEMO_SIGNALS = [
  { id: 1001, symbol: 'BTCUSDT', signal: 'LONG_FORTE', score: 87, probability: 0.72, regime: 'BULL', rsi: 44.2, vol_z: 1.8, upper_wick: 0.012, ret_15: 0.0034, cooldown_min: 0, entry_price: 67420.50, stop_loss: 66950.0, target_price: 68360.5, timestamp: new Date().toISOString(), reasons: ['Regra LONG ativa', 'Confirmação 5m'], meta: { strategy: 'BREAKOUT_MOMENTUM', prob_up: 0.72, prob_down: 0.28, risk: { rr: 2.1 } } },
  { id: 1002, symbol: 'ETHUSDT', signal: 'LONG_FORTE', score: 81, probability: 0.68, regime: 'BULL', rsi: 48.7, vol_z: 2.1, upper_wick: 0.008, ret_15: 0.0021, cooldown_min: 0, entry_price: 3215.80, stop_loss: 3188.0, target_price: 3271.6, timestamp: new Date().toISOString(), reasons: ['Regra LONG ativa', 'Vol-Z elevado'], meta: { strategy: 'TREND_FOLLOW', prob_up: 0.68, prob_down: 0.32, risk: { rr: 2.0 } } },
  { id: 1003, symbol: 'SOLUSDT', signal: 'LONG_FRACO', score: 64, probability: 0.61, regime: 'BULL', rsi: 52.1, vol_z: 0.9, upper_wick: 0.021, ret_15: 0.0015, cooldown_min: 0, entry_price: 148.32, stop_loss: 145.80, target_price: 153.36, timestamp: new Date().toISOString(), reasons: ['RSI neutro favorável'], meta: { strategy: 'MEAN_REVERT', prob_up: 0.61, prob_down: 0.39, risk: { rr: 2.0 } } },
  { id: 1004, symbol: 'XRPUSDT', signal: 'NEUTRO', score: 38, probability: 0.49, regime: 'CHOP', rsi: 50.3, vol_z: 0.3, upper_wick: 0.005, ret_15: -0.0003, cooldown_min: 12, entry_price: 0.5821, stop_loss: 0.5750, target_price: 0.5963, timestamp: new Date().toISOString(), reasons: ['Cooldown ativo', 'Regime CHOP'], meta: { strategy: 'NONE', prob_up: 0.49, prob_down: 0.51, risk: { rr: 1.5 } } },
  { id: 1005, symbol: 'ADAUSDT', signal: 'SHORT_FORTE', score: 78, probability: 0.69, regime: 'BEAR', rsi: 68.4, vol_z: 1.5, upper_wick: 0.041, ret_15: -0.0028, cooldown_min: 0, entry_price: 0.4432, stop_loss: 0.4510, target_price: 0.4276, timestamp: new Date().toISOString(), reasons: ['Regra SHORT ativa', 'RSI sobrecomprado'], meta: { strategy: 'REVERSAL', prob_up: 0.31, prob_down: 0.69, risk: { rr: 2.0 } } },
  { id: 1006, symbol: 'DOGEUSDT', signal: 'NEUTRO', score: 42, probability: 0.51, regime: 'CHOP', rsi: 53.8, vol_z: 0.6, upper_wick: 0.018, ret_15: 0.0007, cooldown_min: 0, entry_price: 0.1528, stop_loss: 0.1495, target_price: 0.1594, timestamp: new Date().toISOString(), reasons: ['Sem confirmação clara'], meta: { strategy: 'NONE', prob_up: 0.51, prob_down: 0.49, risk: { rr: 1.5 } } },
  { id: 1007, symbol: 'AVAXUSDT', signal: 'LONG_FORTE', score: 83, probability: 0.71, regime: 'BULL', rsi: 42.6, vol_z: 2.4, upper_wick: 0.009, ret_15: 0.0041, cooldown_min: 0, entry_price: 38.14, stop_loss: 37.50, target_price: 39.42, timestamp: new Date().toISOString(), reasons: ['Regra LONG ativa', 'Vol-Z extremo'], meta: { strategy: 'BREAKOUT_MOMENTUM', prob_up: 0.71, prob_down: 0.29, risk: { rr: 2.0 } } },
  { id: 1008, symbol: 'DOTUSDT', signal: 'SHORT_FRACO', score: 57, probability: 0.62, regime: 'BEAR', rsi: 65.2, vol_z: 1.1, upper_wick: 0.031, ret_15: -0.0018, cooldown_min: 0, entry_price: 6.721, stop_loss: 6.842, target_price: 6.479, timestamp: new Date().toISOString(), reasons: ['RSI sobrecomprado', 'Upper wick elevado'], meta: { strategy: 'REVERSAL', prob_up: 0.38, prob_down: 0.62, risk: { rr: 2.0 } } },
  { id: 1009, symbol: 'LINKUSDT', signal: 'LONG_FORTE', score: 79, probability: 0.70, regime: 'BULL', rsi: 46.1, vol_z: 1.9, upper_wick: 0.011, ret_15: 0.0029, cooldown_min: 0, entry_price: 14.28, stop_loss: 14.02, target_price: 14.80, timestamp: new Date().toISOString(), reasons: ['Regra LONG ativa'], meta: { strategy: 'TREND_FOLLOW', prob_up: 0.70, prob_down: 0.30, risk: { rr: 2.0 } } },
  { id: 1010, symbol: 'UNIUSDT', signal: 'NEUTRO', score: 33, probability: 0.48, regime: 'CHOP', rsi: 51.0, vol_z: 0.2, upper_wick: 0.007, ret_15: 0.0001, cooldown_min: 0, entry_price: 8.941, stop_loss: 8.780, target_price: 9.263, timestamp: new Date().toISOString(), reasons: ['Regime lateral'], meta: { strategy: 'NONE', prob_up: 0.48, prob_down: 0.52, risk: { rr: 1.5 } } },
];

export interface ScalpTrade {
  id: string;
  symbol: string;
  side: 'LONG' | 'SHORT';
  entryPrice: number;
  exitPrice?: number;
  size: number;
  pnl?: number;
  pnlPct?: number;
  status: 'OPEN' | 'WIN' | 'LOSS';
  holdMs: number;
  ts: number;
}

const makeId = () => Math.random().toString(36).slice(2, 10);

let _tradeIdCounter = 1;

export function generateScalpTrade(): ScalpTrade {
  const symbols = ['BTCUSDT', 'ETHUSDT', 'SOLUSDT', 'AVAXUSDT', 'LINKUSDT', 'BNBUSDT'];
  const prices: Record<string, number> = {
    BTCUSDT: 67420, ETHUSDT: 3216, SOLUSDT: 148.3, AVAXUSDT: 38.14, LINKUSDT: 14.28, BNBUSDT: 598,
  };
  const symbol = symbols[Math.floor(Math.random() * symbols.length)];
  const side = Math.random() > 0.45 ? 'LONG' : 'SHORT';
  const base = prices[symbol];
  const entryPrice = base * (1 + (Math.random() - 0.5) * 0.001);
  const sizes: Record<string, number> = { BTCUSDT: 0.02, ETHUSDT: 0.5, SOLUSDT: 12, AVAXUSDT: 8, LINKUSDT: 40, BNBUSDT: 2 };
  const size = sizes[symbol];

  const win = Math.random() < 0.68;
  const movePct = (Math.random() * 0.003 + 0.001) * (win ? 1 : -1);
  const exitPrice = entryPrice * (1 + (side === 'LONG' ? 1 : -1) * movePct);
  const pnl = (exitPrice - entryPrice) * size * (side === 'LONG' ? 1 : -1);
  const holdMs = Math.floor(Math.random() * 120000 + 15000);

  const id = `T${String(_tradeIdCounter++).padStart(4, '0')}-${makeId()}`;

  return {
    id,
    symbol,
    side,
    entryPrice,
    exitPrice,
    size,
    pnl,
    pnlPct: movePct * (win ? 1 : -1) * 100,
    status: win ? 'WIN' : 'LOSS',
    holdMs,
    ts: Date.now(),
  };
}

// Pre-built history
export const INITIAL_TRADES: ScalpTrade[] = Array.from({ length: 18 }, (_, i) => {
  const t = generateScalpTrade();
  t.ts = Date.now() - (18 - i) * 55000;
  return t;
});

export const DEMO_ALERTS = [
  { id: 2001, message: 'Sinal LONG FORTE em BTCUSDT — Score 87', type: 'NEW_SIGNAL', timestamp: new Date(Date.now() - 45000).toISOString() },
  { id: 2002, message: 'Sinal LONG FORTE em ETHUSDT — Score 81', type: 'NEW_SIGNAL', timestamp: new Date(Date.now() - 120000).toISOString() },
  { id: 2003, message: 'Sinal LONG FORTE em AVAXUSDT — Score 83', type: 'NEW_SIGNAL', timestamp: new Date(Date.now() - 300000).toISOString() },
];

export const DEMO_METRICS = {
  symbols: {
    BTCUSDT:  { hit_rate: 0.71, count: 142 },
    ETHUSDT:  { hit_rate: 0.68, count: 128 },
    SOLUSDT:  { hit_rate: 0.64, count: 89 },
    AVAXUSDT: { hit_rate: 0.72, count: 76 },
    ADAUSDT:  { hit_rate: 0.59, count: 64 },
    LINKUSDT: { hit_rate: 0.66, count: 55 },
    XRPUSDT:  { hit_rate: 0.52, count: 93 },
    DOGEUSDT: { hit_rate: 0.48, count: 70 },
  },
  regimes: {
    BULL: { hit_rate: 0.73, count: 214 },
    BEAR: { hit_rate: 0.65, count: 148 },
    CHOP: { hit_rate: 0.44, count: 189 },
    ALT:  { hit_rate: 0.61, count: 66 },
  },
};

export const DEMO_RISK = {
  SystemStressIndex: 0.22,
  MempoolPressure:   0.12,
  ExecutionFragility: 0.17,
  ExposureRisk:       0.19,
  KillSwitch:         false,
  MarkoutDegradationScore: 0.06,
  AdverseSelectionEMA:     0.028,
};

export const DEMO_ACCOUNT_BASE = {
  Balance:       12450.80,
  Available:      8920.40,
  UsedMargin:     3530.40,
  Leverage:       5,
  UnrealizedPnl:  284.20,
};

export const DEMO_POSITIONS = [
  { Symbol: 'BTCUSDT', Size: 0.0420, AvgPrice: 67350.00, Updated: new Date(Date.now() - 900000).toISOString() },
  { Symbol: 'ETHUSDT', Size: 0.8500, AvgPrice: 3198.50,  Updated: new Date(Date.now() - 1800000).toISOString() },
  { Symbol: 'AVAXUSDT', Size: -12.000, AvgPrice: 38.50,  Updated: new Date(Date.now() - 600000).toISOString() },
];

export const DEMO_LEDGER = [
  { OrderID: 'ord-a1b2c3d4', Symbol: 'BTCUSDT',  Side: 'BUY',  FilledQty: 0.0420, Price: 67350.00, Timestamp: new Date(Date.now() - 900000).toISOString(),  Fee: 0.000084 },
  { OrderID: 'ord-b2c3d4e5', Symbol: 'ETHUSDT',  Side: 'BUY',  FilledQty: 0.8500, Price: 3198.50,  Timestamp: new Date(Date.now() - 1800000).toISOString(), Fee: 0.00085 },
  { OrderID: 'ord-c3d4e5f6', Symbol: 'AVAXUSDT', Side: 'SELL', FilledQty: 12.000, Price: 38.50,    Timestamp: new Date(Date.now() - 600000).toISOString(),  Fee: 0.012 },
  { OrderID: 'ord-d4e5f6a7', Symbol: 'SOLUSDT',  Side: 'BUY',  FilledQty: 5.200,  Price: 146.80,   Timestamp: new Date(Date.now() - 3600000).toISOString(), Fee: 0.0052 },
  { OrderID: 'ord-e5f6a7b8', Symbol: 'SOLUSDT',  Side: 'SELL', FilledQty: 5.200,  Price: 149.40,   Timestamp: new Date(Date.now() - 2700000).toISOString(), Fee: 0.0052 },
  { OrderID: 'ord-f6a7b8c9', Symbol: 'LINKUSDT', Side: 'BUY',  FilledQty: 80.000, Price: 14.10,    Timestamp: new Date(Date.now() - 7200000).toISOString(), Fee: 0.08 },
  { OrderID: 'ord-a7b8c9d0', Symbol: 'LINKUSDT', Side: 'SELL', FilledQty: 80.000, Price: 14.35,    Timestamp: new Date(Date.now() - 6300000).toISOString(), Fee: 0.08 },
];

export const DEMO_EVENTS = [
  { OrderID: 'ord-a1b2c3d4', Symbol: 'BTCUSDT',  Slippage: -0.000012, LatencyMs: 48, Regime: 'BULL' },
  { OrderID: 'ord-b2c3d4e5', Symbol: 'ETHUSDT',  Slippage:  0.000008, LatencyMs: 52, Regime: 'BULL' },
  { OrderID: 'ord-c3d4e5f6', Symbol: 'AVAXUSDT', Slippage: -0.000031, LatencyMs: 61, Regime: 'BEAR' },
  { OrderID: 'ord-d4e5f6a7', Symbol: 'SOLUSDT',  Slippage:  0.000005, LatencyMs: 44, Regime: 'BULL' },
  { OrderID: 'ord-e5f6a7b8', Symbol: 'SOLUSDT',  Slippage: -0.000018, LatencyMs: 57, Regime: 'BULL' },
];

export const DEMO_RECON = {
  status: 'OK', matched: 7, unmatched: 0,
  last_check: new Date().toISOString(),
  details: 'Todos os fills reconciliados com sucesso.',
};

export function livePrice(base: number, vol = 0.001) {
  return base * (1 + (Math.random() - 0.5) * vol);
}
