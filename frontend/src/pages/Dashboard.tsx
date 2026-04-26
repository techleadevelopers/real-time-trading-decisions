import { useEffect, useRef, useState, useCallback } from 'react';
import {
  Area, AreaChart, ResponsiveContainer, Tooltip,
  XAxis, YAxis,
} from 'recharts';
import { TrendingUp, TrendingDown, Zap, Activity, Target, BarChart2, Clock, Cpu } from 'lucide-react';
import { usePolling } from '../hooks/useQuery';
import { fetchSignals, fetchCPAccount } from '../api/backend';
import { INITIAL_TRADES, generateScalpTrade, type ScalpTrade } from '../api/demo';

// ── Balance chart seed ──
function buildBalanceSeries(base: number) {
  const pts = [];
  let val = base - 380;
  const now = Date.now();
  for (let i = 40; i >= 0; i--) {
    val += (Math.random() - 0.38) * 18;
    pts.push({ t: new Date(now - i * 90000).toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' }), v: parseFloat(val.toFixed(2)) });
  }
  pts[pts.length - 1].v = base;
  return pts;
}

// ── PnL Stats Panel ──
function PnlStatsPanel({ trades, sessionPnl, wins, tradeCount }: { trades: ScalpTrade[]; sessionPnl: number; wins: number; tradeCount: number }) {
  const losses = tradeCount - wins;
  const winRate = tradeCount > 0 ? (wins / tradeCount) * 100 : 0;

  const closedTrades = trades.filter((t) => t.status !== 'OPEN' && t.pnl != null);
  const winTrades = closedTrades.filter((t) => (t.pnl ?? 0) > 0);
  const lossTrades = closedTrades.filter((t) => (t.pnl ?? 0) <= 0);
  const avgWin = winTrades.length ? winTrades.reduce((s, t) => s + (t.pnl ?? 0), 0) / winTrades.length : 0;
  const avgLoss = lossTrades.length ? lossTrades.reduce((s, t) => s + (t.pnl ?? 0), 0) / lossTrades.length : 0;
  const bestTrade = closedTrades.reduce((best, t) => (t.pnl ?? 0) > (best.pnl ?? -Infinity) ? t : best, closedTrades[0] ?? null);
  const worstTrade = closedTrades.reduce((worst, t) => (t.pnl ?? 0) < (worst.pnl ?? Infinity) ? t : worst, closedTrades[0] ?? null);
  const avgHoldMs = closedTrades.length ? closedTrades.reduce((s, t) => s + t.holdMs, 0) / closedTrades.length : 0;
  const avgHoldStr = avgHoldMs < 60000 ? `${Math.round(avgHoldMs / 1000)}s` : `${Math.round(avgHoldMs / 60000)}m`;

  const pnlColor = sessionPnl >= 0 ? 'var(--green)' : 'var(--red)';

  // Last 8 trades as colored dots
  const last8 = [...trades].reverse().slice(0, 8).reverse();

  return (
    <div className="card card-glow">
      <div className="card-header">
        <span className="card-title"><Activity size={11} />Estatísticas de PnL — Sessão</span>
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 13, fontWeight: 800, color: pnlColor, textShadow: `0 0 12px ${pnlColor}88` }}>
          {sessionPnl >= 0 ? '+' : ''}{sessionPnl.toFixed(2)} $
        </span>
      </div>

      {/* Win/Loss bar */}
      <div style={{ marginBottom: 14 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 10, color: 'var(--text-dim)', marginBottom: 5 }}>
          <span style={{ color: 'var(--green)', fontFamily: 'var(--font-mono)', fontWeight: 700 }}>✓ {wins} vitórias</span>
          <span style={{ fontFamily: 'var(--font-mono)', fontWeight: 700, color: winRate >= 60 ? 'var(--green)' : 'var(--yellow)' }}>{winRate.toFixed(1)}%</span>
          <span style={{ color: 'var(--red)', fontFamily: 'var(--font-mono)', fontWeight: 700 }}>{losses} perdas ✗</span>
        </div>
        <div style={{ height: 6, background: 'rgba(255,255,255,0.05)', borderRadius: 3, overflow: 'hidden', display: 'flex' }}>
          <div style={{ width: `${winRate}%`, background: 'linear-gradient(90deg, #00ff88, #00c8ff)', borderRadius: '3px 0 0 3px', transition: 'width 0.5s' }} />
          <div style={{ flex: 1, background: 'rgba(255,0,85,0.4)', borderRadius: '0 3px 3px 0' }} />
        </div>
      </div>

      {/* Stats grid */}
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8, marginBottom: 14 }}>
        {[
          { label: 'Média Vitória', value: `+${avgWin.toFixed(2)}$`, color: 'var(--green)' },
          { label: 'Média Perda',   value: `${avgLoss.toFixed(2)}$`,  color: 'var(--red)' },
          { label: 'Melhor Trade',  value: bestTrade  ? `+${(bestTrade.pnl  ?? 0).toFixed(2)}$` : '—', color: 'var(--green)' },
          { label: 'Pior Trade',    value: worstTrade ? `${(worstTrade.pnl ?? 0).toFixed(2)}$`  : '—', color: 'var(--red)' },
        ].map((item) => (
          <div key={item.label} style={{ background: 'rgba(255,255,255,0.02)', border: '1px solid var(--border)', borderRadius: 7, padding: '8px 10px' }}>
            <div style={{ fontSize: 9, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.08em', marginBottom: 3 }}>{item.label}</div>
            <div style={{ fontFamily: 'var(--font-mono)', fontSize: 13, fontWeight: 700, color: item.color }}>{item.value}</div>
          </div>
        ))}
      </div>

      {/* Hold time + total */}
      <div className="metric-row">
        <span className="metric-label"><Clock size={10} /> Hold médio</span>
        <span className="metric-value c-cyan">{avgHoldStr}</span>
      </div>
      <div className="metric-row">
        <span className="metric-label">Total de operações</span>
        <span className="metric-value">{tradeCount}</span>
      </div>

      {/* Last 8 trades dot strip */}
      <div style={{ marginTop: 12 }}>
        <div style={{ fontSize: 9, color: 'var(--text-muted)', marginBottom: 6, textTransform: 'uppercase', letterSpacing: '0.08em' }}>Últimas operações</div>
        <div style={{ display: 'flex', gap: 5, alignItems: 'center' }}>
          {last8.map((t) => {
            const win = (t.pnl ?? 0) > 0;
            return (
              <div key={t.id} title={`${t.symbol} ${win ? '+' : ''}${(t.pnl ?? 0).toFixed(2)}$`}
                style={{
                  width: 22, height: 22, borderRadius: 5,
                  background: win ? 'rgba(0,255,136,0.15)' : 'rgba(255,0,85,0.15)',
                  border: `1px solid ${win ? 'rgba(0,255,136,0.4)' : 'rgba(255,0,85,0.4)'}`,
                  display: 'flex', alignItems: 'center', justifyContent: 'center',
                  fontSize: 9, fontWeight: 700, color: win ? 'var(--green)' : 'var(--red)',
                  fontFamily: 'var(--font-mono)',
                }}>
                {win ? 'W' : 'L'}
              </div>
            );
          })}
          {last8.length < 8 && Array.from({ length: 8 - last8.length }).map((_, i) => (
            <div key={`empty-${i}`} style={{ width: 22, height: 22, borderRadius: 5, background: 'rgba(255,255,255,0.03)', border: '1px solid var(--border)' }} />
          ))}
        </div>
      </div>
    </div>
  );
}

const NeonTooltip = ({ active, payload }: { active?: boolean; payload?: { value: number }[] }) => {
  if (!active || !payload?.length) return null;
  const v = payload[0].value;
  return (
    <div style={{ background: 'rgba(4,10,22,0.95)', border: '1px solid rgba(0,200,255,0.3)', borderRadius: 6, padding: '6px 12px', fontFamily: 'var(--font-mono)', fontSize: 11 }}>
      <span style={{ color: v >= 0 ? 'var(--green)' : 'var(--red)' }}>{v >= 0 ? '+' : ''}{v.toFixed(2)} USDT</span>
    </div>
  );
};

// ── Live Scalp Feed ──
function ScalpFeed({ trades }: { trades: ScalpTrade[] }) {
  const msToStr = (ms: number) => ms < 60000 ? `${Math.round(ms / 1000)}s` : `${Math.round(ms / 60000)}m`;
  return (
    <div className="scalp-feed">
      {[...trades].reverse().slice(0, 20).map((t, i) => {
        const isLong = t.side === 'LONG';
        const pnlColor = (t.pnl ?? 0) >= 0 ? 'var(--green)' : 'var(--red)';
        return (
          <div key={t.id} className={`scalp-trade ${isLong ? 'long' : 'short'}${t.status !== 'OPEN' ? ' closed' : ''}`}
            style={{ animationDelay: `${i * 0.03}s` }}>
            {/* Side icon */}
            <div style={{ flexShrink: 0 }}>
              {isLong
                ? <TrendingUp size={12} color="var(--green)" />
                : <TrendingDown size={12} color="var(--red)" />}
            </div>
            {/* Symbol */}
            <span style={{ color: 'var(--text)', fontWeight: 700, minWidth: 70 }}>{t.symbol}</span>
            {/* Side badge */}
            <span className={`badge ${isLong ? 'badge-green' : 'badge-red'}`} style={{ minWidth: 42, justifyContent: 'center' }}>
              {isLong ? 'LONG' : 'SHORT'}
            </span>
            {/* Entry */}
            <span style={{ color: 'var(--text-dim)', minWidth: 70 }}>{t.entryPrice.toFixed(t.entryPrice > 100 ? 2 : 4)}</span>
            {/* Arrow */}
            <span style={{ color: 'var(--text-muted)' }}>→</span>
            {/* Exit */}
            {t.exitPrice
              ? <span style={{ color: pnlColor, minWidth: 70 }}>{t.exitPrice.toFixed(t.exitPrice > 100 ? 2 : 4)}</span>
              : <span style={{ color: 'var(--yellow)' }}>ABERTA</span>}
            {/* PnL */}
            <span style={{ color: pnlColor, fontWeight: 700, marginLeft: 'auto', minWidth: 68, textAlign: 'right' }}>
              {t.pnl != null ? `${t.pnl >= 0 ? '+' : ''}${t.pnl.toFixed(2)} $` : '—'}
            </span>
            {/* Hold */}
            <span style={{ color: 'var(--text-muted)', minWidth: 28, textAlign: 'right' }}><Clock size={9} style={{ display: 'inline' }} /> {msToStr(t.holdMs)}</span>
            {/* Status */}
            {t.status === 'WIN' && <span style={{ color: 'var(--green)', fontSize: 9, minWidth: 20 }}>✓</span>}
            {t.status === 'LOSS' && <span style={{ color: 'var(--red)', fontSize: 9, minWidth: 20 }}>✗</span>}
            {t.status === 'OPEN' && <div className="live-dot live-dot-yellow" style={{ width: 5, height: 5 }} />}
          </div>
        );
      })}
    </div>
  );
}

export default function Dashboard() {
  const { data: signals } = usePolling(fetchSignals, 14000);
  const { data: account } = usePolling(fetchCPAccount, 8000);

  const [trades, setTrades] = useState<ScalpTrade[]>(INITIAL_TRADES);
  const [balanceSeries, setBalanceSeries] = useState(() => buildBalanceSeries(12450.80));
  const [sessionPnl, setSessionPnl] = useState(0);
  const [tradeCount, setTradeCount] = useState(INITIAL_TRADES.length);
  const [wins, setWins] = useState(INITIAL_TRADES.filter((t) => t.status === 'WIN').length);
  const [botTime, setBotTime] = useState(0);
  const tickRef = useRef(0);

  // Calculate from initial
  useEffect(() => {
    const initPnl = INITIAL_TRADES.reduce((a, t) => a + (t.pnl ?? 0), 0);
    setSessionPnl(initPnl);
  }, []);

  // Bot timer
  useEffect(() => {
    const t = setInterval(() => setBotTime((v) => v + 1), 1000);
    return () => clearInterval(t);
  }, []);

  // Generate new scalp trades periodically
  const addTrade = useCallback(() => {
    const t = generateScalpTrade();
    setTrades((prev) => [...prev.slice(-50), t]);
    setSessionPnl((v) => v + (t.pnl ?? 0));
    setTradeCount((v) => v + 1);
    if (t.status === 'WIN') setWins((v) => v + 1);

    // Update balance chart
    const balanceDelta = t.pnl ?? 0;
    setBalanceSeries((prev) => {
      const last = prev[prev.length - 1];
      const now = new Date().toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' });
      return [...prev.slice(-39), { t: now, v: parseFloat((last.v + balanceDelta).toFixed(2)) }];
    });
  }, []);

  useEffect(() => {
    const interval = Math.floor(Math.random() * 4000) + 6000;
    const t = setInterval(() => {
      tickRef.current++;
      addTrade();
    }, interval);
    return () => clearInterval(t);
  }, [addTrade]);

  const sigList = (signals as Signal[]) || [];
  const topSigs = [...sigList].sort((a, b) => b.score - a.score).filter((s) => s.signal !== 'NEUTRO').slice(0, 5);
  const winRate = tradeCount > 0 ? ((wins / tradeCount) * 100).toFixed(1) : '0.0';
  const acc = account as AccountState | null;
  const balance = acc?.Balance ?? 12450.80;
  const fmtTime = (s: number) => {
    const h = Math.floor(s / 3600).toString().padStart(2, '0');
    const m = Math.floor((s % 3600) / 60).toString().padStart(2, '0');
    const ss = (s % 60).toString().padStart(2, '0');
    return `${h}:${m}:${ss}`;
  };

  return (
    <div>
      {/* ── Bot Status Banner ── */}
      <div className="bot-banner">
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <div className="live-dot" />
          <span style={{ fontSize: 12, fontWeight: 700, color: 'var(--green)', fontFamily: 'var(--font-mono)', letterSpacing: '0.08em' }}>
            BOT ATIVO
          </span>
          <span className="badge badge-cyan">SCALP FUTUROS BINGX</span>
        </div>
        <div style={{ display: 'flex', gap: 28, marginLeft: 'auto', flexWrap: 'wrap' }}>
          <Stat label="SESSÃO" value={fmtTime(botTime)} color="var(--cyan)" icon={<Clock size={11} />} />
          <Stat label="TRADES" value={String(tradeCount)} color="var(--cyan)" icon={<Activity size={11} />} />
          <Stat label="WIN RATE" value={`${winRate}%`} color={parseFloat(winRate) >= 60 ? 'var(--green)' : 'var(--yellow)'} icon={<Target size={11} />} />
          <Stat label="PnL SESSÃO" value={`${sessionPnl >= 0 ? '+' : ''}${sessionPnl.toFixed(2)} $`} color={sessionPnl >= 0 ? 'var(--green)' : 'var(--red)'} icon={<BarChart2 size={11} />} />
        </div>
      </div>

      {/* ── KPI row ── */}
      <div className="grid-4">
        <div className="kpi-card neon-border-green">
          <div className="kpi-label"><TrendingUp size={11} />Saldo BingX</div>
          <div className="kpi-value kpi-glow-green">${balance.toLocaleString('pt-BR', { minimumFractionDigits: 2 })}</div>
          <div className="kpi-sub">USDT disponível</div>
          <div className="kpi-accent kpi-accent-green" />
        </div>
        <div className="kpi-card">
          <div className="kpi-label"><Target size={11} />Win Rate</div>
          <div className="kpi-value" style={{ color: parseFloat(winRate) >= 60 ? 'var(--green)' : 'var(--yellow)', textShadow: `0 0 20px ${parseFloat(winRate) >= 60 ? 'rgba(0,255,136,0.7)' : 'rgba(255,187,0,0.7)'}` }}>
            {winRate}%
          </div>
          <div className="kpi-sub">{wins} vitórias / {tradeCount} trades</div>
          <div className="kpi-accent" style={{ background: parseFloat(winRate) >= 60 ? 'linear-gradient(90deg,transparent,var(--green),transparent)' : 'linear-gradient(90deg,transparent,var(--yellow),transparent)' }} />
        </div>
        <div className="kpi-card">
          <div className="kpi-label"><Zap size={11} />Regime Atual</div>
          <div className="kpi-value kpi-glow-cyan">BULL</div>
          <div className="kpi-sub">BTC dominância 52.4%</div>
          <div className="kpi-accent" />
        </div>
        <div className="kpi-card">
          <div className="kpi-label"><Cpu size={11} />Latência RTTS</div>
          <div className="kpi-value kpi-glow-purple">48µs</div>
          <div className="kpi-sub">motor Rust ativo</div>
          <div className="kpi-accent kpi-accent-purple" />
        </div>
      </div>

      {/* ── Balance chart — full width ── */}
      <div className="card card-glow" style={{ marginBottom: 14 }}>
        <div className="card-header">
          <span className="card-title"><BarChart2 size={11} />Curva de Saldo — Sessão</span>
          <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
            {/* Compact PnL result */}
            <div style={{
              padding: '3px 10px', borderRadius: 6,
              background: sessionPnl >= 0 ? 'rgba(0,255,136,0.08)' : 'rgba(255,0,85,0.08)',
              border: `1px solid ${sessionPnl >= 0 ? 'rgba(0,255,136,0.25)' : 'rgba(255,0,85,0.25)'}`,
              fontFamily: 'var(--font-mono)', fontSize: 12, fontWeight: 700,
              color: sessionPnl >= 0 ? 'var(--green)' : 'var(--red)',
            }}>
              PnL&nbsp;{sessionPnl >= 0 ? '+' : ''}{sessionPnl.toFixed(2)} $
            </div>
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--green)' }}>${balance.toFixed(2)}</span>
          </div>
        </div>
        <div style={{ height: 160 }}>
          <ResponsiveContainer width="100%" height="100%">
            <AreaChart data={balanceSeries} margin={{ top: 5, right: 0, left: -20, bottom: 0 }}>
              <defs>
                <linearGradient id="balGrad" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="5%" stopColor="#00ff88" stopOpacity={0.25} />
                  <stop offset="95%" stopColor="#00ff88" stopOpacity={0} />
                </linearGradient>
              </defs>
              <XAxis dataKey="t" tick={{ fontSize: 9, fill: 'var(--text-muted)' }} tickLine={false} axisLine={false} interval="preserveStartEnd" />
              <YAxis tick={{ fontSize: 9, fill: 'var(--text-muted)' }} tickLine={false} axisLine={false} domain={['auto', 'auto']} />
              <Tooltip content={<NeonTooltip />} />
              <Area type="monotone" dataKey="v" stroke="#00ff88" strokeWidth={2} fill="url(#balGrad)"
                dot={false}
                style={{ filter: 'drop-shadow(0 0 6px rgba(0,255,136,0.6))' }} />
            </AreaChart>
          </ResponsiveContainer>
        </div>
      </div>

      {/* ── Scalp Feed + Top Signals ── */}
      <div style={{ display: 'grid', gridTemplateColumns: '1.6fr 1fr', gap: 14 }}>
        {/* Live scalp feed */}
        <div className="card">
          <div className="card-header">
            <span className="card-title">
              <div className="live-dot" style={{ width: 6, height: 6 }} />
              Feed de Micro-Scalp Ao Vivo
            </span>
            <span style={{ fontSize: 10, color: 'var(--text-dim)', fontFamily: 'var(--font-mono)' }}>
              {tradeCount} ordens | atualizando...
            </span>
          </div>
          {/* Header labels */}
          <div style={{ display: 'flex', gap: 10, padding: '4px 12px 8px', fontSize: 9, color: 'var(--text-muted)', fontFamily: 'var(--font-mono)', textTransform: 'uppercase', letterSpacing: '0.08em', borderBottom: '1px solid var(--border)', marginBottom: 8 }}>
            <span style={{ width: 12 }}></span>
            <span style={{ minWidth: 70 }}>ATIVO</span>
            <span style={{ minWidth: 42 }}>LADO</span>
            <span style={{ minWidth: 70 }}>ENTRADA</span>
            <span style={{ width: 14 }}></span>
            <span style={{ minWidth: 70 }}>SAÍDA</span>
            <span style={{ marginLeft: 'auto', minWidth: 68, textAlign: 'right' }}>PnL</span>
            <span style={{ minWidth: 28, textAlign: 'right' }}>HOLD</span>
            <span style={{ minWidth: 20 }}></span>
          </div>
          <ScalpFeed trades={trades} />
        </div>

        {/* Top Signals */}
        <div className="card">
          <div className="card-header">
            <span className="card-title"><Zap size={11} />Sinais Ativos</span>
            <span className="badge badge-cyan">AO VIVO</span>
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            {topSigs.length === 0
              ? <div className="empty">Calculando sinais...</div>
              : topSigs.map((s) => {
                const isLong = s.signal.startsWith('LONG');
                const isStrong = s.signal.endsWith('FORTE');
                const color = isLong ? 'var(--green)' : 'var(--red)';
                const base = s.symbol.replace('USDT', '').toLowerCase();
                const iconUrl = `https://assets.coincap.io/assets/icons/${base}@2x.png`;
                return (
                  <div key={s.id} style={{
                    padding: '9px 12px', borderRadius: 8,
                    background: isLong ? 'rgba(0,255,136,0.04)' : 'rgba(255,0,85,0.04)',
                    border: `1px solid ${isLong ? 'rgba(0,255,136,0.15)' : 'rgba(255,0,85,0.15)'}`,
                    display: 'flex', alignItems: 'center', gap: 10,
                  }}>
                    {/* Token icon */}
                    <div style={{ position: 'relative', flexShrink: 0 }}>
                      <img
                        src={iconUrl}
                        alt={base}
                        width={28} height={28}
                        style={{ borderRadius: '50%', display: 'block', background: 'rgba(255,255,255,0.05)' }}
                        onError={(e) => {
                          const el = e.currentTarget;
                          el.style.display = 'none';
                          const fb = el.nextElementSibling as HTMLElement;
                          if (fb) fb.style.display = 'flex';
                        }}
                      />
                      {/* Fallback initial */}
                      <div style={{
                        display: 'none', width: 28, height: 28, borderRadius: '50%',
                        background: `${color}22`, border: `1px solid ${color}44`,
                        alignItems: 'center', justifyContent: 'center',
                        fontSize: 10, fontWeight: 800, color, fontFamily: 'var(--font-mono)',
                        position: 'absolute', top: 0, left: 0,
                      }}>
                        {base.slice(0, 2).toUpperCase()}
                      </div>
                      {/* Direction dot overlay */}
                      <div style={{
                        position: 'absolute', bottom: -1, right: -1,
                        width: 10, height: 10, borderRadius: '50%',
                        background: color, border: '1.5px solid var(--bg-card-solid)',
                        boxShadow: `0 0 5px ${color}`,
                      }} />
                    </div>

                    {/* Symbol + badge */}
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 4 }}>
                        <span style={{ fontFamily: 'var(--font-mono)', fontWeight: 700, fontSize: 12, color: 'var(--text)' }}>
                          {base.toUpperCase()}
                        </span>
                        <span className={`badge ${isLong ? 'badge-green' : 'badge-red'}`} style={{ fontSize: 9 }}>
                          {isLong ? '▲' : '▼'} {isStrong ? 'FORTE' : 'FRACO'}
                        </span>
                      </div>
                      {/* Score bar */}
                      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                        <div style={{ flex: 1, height: 3, background: 'rgba(255,255,255,0.05)', borderRadius: 2, overflow: 'hidden' }}>
                          <div style={{ height: '100%', width: `${s.score}%`, background: color, borderRadius: 2, boxShadow: `0 0 5px ${color}88`, transition: 'width 0.5s' }} />
                        </div>
                        <span style={{ fontSize: 9, color: 'var(--text-dim)', fontFamily: 'var(--font-mono)', minWidth: 20 }}>{s.score}</span>
                      </div>
                    </div>

                    {/* Probability */}
                    <div style={{ textAlign: 'right', flexShrink: 0 }}>
                      <div style={{ fontFamily: 'var(--font-mono)', fontSize: 13, fontWeight: 700, color, textShadow: `0 0 8px ${color}66` }}>
                        {(s.probability * 100).toFixed(1)}%
                      </div>
                      <div style={{ fontSize: 9, color: 'var(--text-muted)' }}>prob</div>
                    </div>
                  </div>
                );
              })}
          </div>
        </div>
      </div>
    </div>
  );
}

function Stat({ label, value, color, icon }: { label: string; value: string; color: string; icon: React.ReactNode }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
      <div style={{ color: 'var(--text-dim)' }}>{icon}</div>
      <div>
        <div style={{ fontSize: 9, color: 'var(--text-dim)', fontFamily: 'var(--font-mono)', letterSpacing: '0.1em', textTransform: 'uppercase' }}>{label}</div>
        <div style={{ fontFamily: 'var(--font-mono)', fontWeight: 700, fontSize: 13, color, textShadow: `0 0 10px ${color}88` }}>{value}</div>
      </div>
    </div>
  );
}

interface Signal { id: number; symbol: string; signal: string; score: number; probability: number; regime: string; entry_price: number | null; }
interface AccountState { Balance?: number; Available?: number; }
