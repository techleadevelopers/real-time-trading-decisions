import { useState } from 'react';
import { TrendingUp, TrendingDown, Minus, RefreshCw, Filter, Clock } from 'lucide-react';
import { usePolling } from '../hooks/useQuery';
import { fetchSignals } from '../api/backend';

interface Signal {
  id: number;
  symbol: string;
  signal: string;
  score: number;
  probability: number;
  regime: string;
  rsi: number | null;
  vol_z: number | null;
  entry_price: number | null;
  stop_loss: number | null;
  target_price: number | null;
  cooldown_min: number | null;
  reasons: string[];
  timestamp: string;
  meta?: { strategy: string; prob_up: number; prob_down: number; risk: { rr: number | null } };
}

function fmt(n: number | null | undefined, dec = 4) {
  if (n == null) return '—';
  return n.toFixed(dec);
}

function fmtPrice(n: number | null | undefined) {
  if (n == null) return '—';
  return n > 100 ? n.toFixed(2) : n.toFixed(4);
}

function tokenIcon(symbol: string) {
  const base = symbol.replace('USDT', '').toLowerCase();
  return `https://assets.coincap.io/assets/icons/${base}@2x.png`;
}

function SignalRow({ s, index }: { s: Signal; index: number }) {
  const isLong = s.signal.startsWith('LONG');
  const isShort = s.signal.startsWith('SHORT');
  const isNeutro = !isLong && !isShort;
  const isStrong = s.signal.endsWith('FORTE');

  const color = isLong ? 'var(--green)' : isShort ? 'var(--red)' : 'var(--text-dim)';
  const bgColor = isLong
    ? 'rgba(0,255,136,0.05)'
    : isShort
    ? 'rgba(255,0,85,0.05)'
    : 'rgba(255,255,255,0.02)';
  const borderColor = isLong
    ? 'rgba(0,255,136,0.18)'
    : isShort
    ? 'rgba(255,0,85,0.18)'
    : 'rgba(255,255,255,0.06)';

  const base = s.symbol.replace('USDT', '').toLowerCase();

  // Calculate expected PnL direction and target gain
  const hasEntry = s.entry_price != null && s.target_price != null;
  const expectedGain = hasEntry
    ? Math.abs((s.target_price! - s.entry_price!) * (isLong ? 1 : -1))
    : null;

  const cooldownActive = (s.cooldown_min ?? 0) > 0;

  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 10,
        padding: '9px 14px',
        borderRadius: 7,
        background: bgColor,
        border: `1px solid ${borderColor}`,
        position: 'relative',
        overflow: 'hidden',
        animation: `fadeSlideIn 0.3s ease both`,
        animationDelay: `${index * 0.04}s`,
        fontFamily: 'var(--font-mono)',
        fontSize: 11,
      }}
    >
      {/* Left accent bar */}
      <div style={{
        position: 'absolute', left: 0, top: 0, bottom: 0, width: 3,
        background: isNeutro ? 'transparent' : color,
        boxShadow: isNeutro ? 'none' : `0 0 8px ${color}`,
      }} />

      {/* Token icon */}
      <div style={{ position: 'relative', flexShrink: 0, marginLeft: 4 }}>
        <img
          src={tokenIcon(s.symbol)}
          alt={base}
          width={24} height={24}
          style={{ borderRadius: '50%', display: 'block', background: 'rgba(255,255,255,0.04)' }}
          onError={(e) => {
            const el = e.currentTarget;
            el.style.display = 'none';
            const fb = el.nextElementSibling as HTMLElement;
            if (fb) fb.style.display = 'flex';
          }}
        />
        <div style={{
          display: 'none', width: 24, height: 24, borderRadius: '50%',
          background: `${color}22`, border: `1px solid ${color}44`,
          alignItems: 'center', justifyContent: 'center',
          fontSize: 8, fontWeight: 800, color,
          position: 'absolute', top: 0, left: 0,
        }}>
          {base.slice(0, 2).toUpperCase()}
        </div>
        {!isNeutro && (
          <div style={{
            position: 'absolute', bottom: -1, right: -1,
            width: 8, height: 8, borderRadius: '50%',
            background: color, border: '1.5px solid #020408',
            boxShadow: `0 0 4px ${color}`,
          }} />
        )}
      </div>

      {/* Symbol */}
      <span style={{ fontWeight: 700, fontSize: 12, color: 'var(--text)', minWidth: 72 }}>
        {s.symbol.replace('USDT', '')}
        <span style={{ color: 'var(--text-dim)', fontWeight: 400 }}>/USDT</span>
      </span>

      {/* Side badge */}
      <div style={{ minWidth: 80 }}>
        {isLong && (
          <span className={`badge badge-green`} style={{ fontSize: 9 }}>
            <TrendingUp size={9} />{isStrong ? 'LONG FORTE' : 'LONG'}
          </span>
        )}
        {isShort && (
          <span className={`badge badge-red`} style={{ fontSize: 9 }}>
            <TrendingDown size={9} />{isStrong ? 'SHORT FORTE' : 'SHORT'}
          </span>
        )}
        {isNeutro && (
          <span className="badge badge-gray" style={{ fontSize: 9 }}>
            <Minus size={9} />NEUTRO
          </span>
        )}
      </div>

      {/* Entry */}
      <span style={{ color: 'var(--text-dim)', minWidth: 76 }}>{fmtPrice(s.entry_price)}</span>

      {/* Arrow */}
      <span style={{ color: 'var(--text-muted)' }}>→</span>

      {/* Target */}
      <span style={{ color: isNeutro ? 'var(--text-muted)' : color, minWidth: 76 }}>
        {fmtPrice(s.target_price)}
      </span>

      {/* Stop */}
      <span style={{ color: 'var(--red)', minWidth: 76 }}>{fmtPrice(s.stop_loss)}</span>

      {/* Expected gain */}
      <span style={{
        color: isNeutro ? 'var(--text-muted)' : color,
        fontWeight: 700,
        minWidth: 50,
      }}>
        {expectedGain != null ? `~${expectedGain.toFixed(2)}$` : '—'}
      </span>

      {/* Score bar + value */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 5, minWidth: 70 }}>
        <div style={{ flex: 1, height: 3, background: 'rgba(255,255,255,0.05)', borderRadius: 2, overflow: 'hidden' }}>
          <div style={{
            height: '100%',
            width: `${s.score}%`,
            background: s.score >= 70 ? 'var(--green)' : s.score >= 50 ? 'var(--cyan)' : 'var(--text-muted)',
            borderRadius: 2,
            boxShadow: s.score >= 70 ? '0 0 5px rgba(0,255,136,0.6)' : 'none',
          }} />
        </div>
        <span style={{
          color: s.score >= 70 ? 'var(--green)' : s.score >= 50 ? 'var(--cyan)' : 'var(--text-muted)',
          minWidth: 20, textAlign: 'right',
        }}>{s.score}</span>
      </div>

      {/* Prob */}
      <span style={{ color, marginLeft: 'auto', minWidth: 42, textAlign: 'right', fontWeight: 700 }}>
        {(s.probability * 100).toFixed(1)}%
      </span>

      {/* Status icon */}
      <div style={{ minWidth: 20, textAlign: 'center' }}>
        {cooldownActive
          ? <span style={{ color: 'var(--yellow)', fontSize: 9 }}><Clock size={10} /></span>
          : isNeutro
          ? <span style={{ color: 'var(--text-muted)', fontSize: 9 }}>—</span>
          : <span style={{ color, fontSize: 11 }}>{isLong ? '▲' : '▼'}</span>}
      </div>
    </div>
  );
}

export default function Sinais() {
  const [filterSignal, setFilterSignal] = useState('TODOS');
  const [onlyStrong, setOnlyStrong] = useState(false);
  const [minScore, setMinScore] = useState(0);

  const { data, loading, refetch } = usePolling(
    () => fetchSignals(minScore, onlyStrong),
    10000
  );

  const signals: Signal[] = (data as Signal[] | null) || [];
  const filtered = signals.filter((s) => {
    if (filterSignal === 'LONG') return s.signal?.startsWith('LONG');
    if (filterSignal === 'SHORT') return s.signal?.startsWith('SHORT');
    if (filterSignal === 'NEUTRO') return s.signal === 'NEUTRO';
    return true;
  });

  const longs = signals.filter((s) => s.signal.startsWith('LONG')).length;
  const shorts = signals.filter((s) => s.signal.startsWith('SHORT')).length;
  const neutros = signals.filter((s) => s.signal === 'NEUTRO').length;

  return (
    <div>
      {/* Stats row */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 12, marginBottom: 14 }}>
        {[
          { label: 'Total Ativos', value: signals.length, color: 'var(--cyan)' },
          { label: 'LONG Ativos', value: longs, color: 'var(--green)' },
          { label: 'SHORT Ativos', value: shorts, color: 'var(--red)' },
          { label: 'Neutros', value: neutros, color: 'var(--text-dim)' },
        ].map((item) => (
          <div key={item.label} className="kpi-card" style={{ padding: '12px 16px' }}>
            <div className="kpi-label">{item.label}</div>
            <div style={{ fontFamily: 'var(--font-mono)', fontSize: 22, fontWeight: 800, color: item.color,
              textShadow: `0 0 16px ${item.color}88` }}>{item.value}</div>
          </div>
        ))}
      </div>

      {/* Filters + feed card */}
      <div className="card">
        {/* Header */}
        <div className="card-header" style={{ marginBottom: 12 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <div className="live-dot" style={{ width: 6, height: 6 }} />
            <span className="card-title">Feed de Sinais — Ao Vivo</span>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <span style={{ fontSize: 10, color: 'var(--text-dim)', fontFamily: 'var(--font-mono)' }}>
              {loading ? 'atualizando...' : `${filtered.length} sinais`}
            </span>
            <button className="btn btn-ghost" style={{ padding: '4px 10px' }} onClick={refetch} disabled={loading}>
              <RefreshCw size={11} style={{ animation: loading ? 'spin 1s linear infinite' : 'none' }} />
            </button>
          </div>
        </div>

        {/* Filter bar */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 12, flexWrap: 'wrap' }}>
          <Filter size={11} color="var(--text-dim)" />
          <div className="tabs" style={{ width: 'auto' }}>
            {['TODOS', 'LONG', 'SHORT', 'NEUTRO'].map((f) => (
              <button key={f} className={`tab-btn${filterSignal === f ? ' active' : ''}`}
                onClick={() => setFilterSignal(f)} style={{ padding: '4px 10px', fontSize: 10 }}>
                {f}
              </button>
            ))}
          </div>
          <label style={{ display: 'flex', alignItems: 'center', gap: 5, fontSize: 11, color: 'var(--text-dim)', cursor: 'pointer' }}>
            <input type="checkbox" checked={onlyStrong} onChange={(e) => setOnlyStrong(e.target.checked)}
              style={{ accentColor: 'var(--cyan)', width: 12, height: 12 }} />
            Só fortes
          </label>
          <div style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
            <span style={{ fontSize: 10, color: 'var(--text-dim)' }}>Score ≥</span>
            <input type="number" value={minScore} onChange={(e) => setMinScore(Number(e.target.value))}
              min={0} max={100}
              style={{ width: 46, background: 'rgba(0,200,255,0.05)', border: '1px solid var(--border)',
                borderRadius: 5, padding: '3px 7px', color: 'var(--text)', fontFamily: 'var(--font-mono)',
                fontSize: 11, outline: 'none' }} />
          </div>
        </div>

        {/* Column headers */}
        <div style={{
          display: 'flex', gap: 10, padding: '5px 14px 8px',
          fontSize: 9, color: 'var(--text-muted)', fontFamily: 'var(--font-mono)',
          textTransform: 'uppercase', letterSpacing: '0.08em',
          borderBottom: '1px solid var(--border)', marginBottom: 8,
        }}>
          <span style={{ width: 4 }} />
          <span style={{ width: 24 }} />
          <span style={{ minWidth: 72 }}>ATIVO</span>
          <span style={{ minWidth: 80 }}>LADO</span>
          <span style={{ minWidth: 76 }}>ENTRADA</span>
          <span style={{ width: 14 }} />
          <span style={{ minWidth: 76 }}>ALVO</span>
          <span style={{ minWidth: 76 }}>STOP</span>
          <span style={{ minWidth: 50 }}>GANHO EST.</span>
          <span style={{ minWidth: 70 }}>SCORE</span>
          <span style={{ marginLeft: 'auto', minWidth: 42, textAlign: 'right' }}>PROB</span>
          <span style={{ minWidth: 20 }} />
        </div>

        {/* Rows */}
        {loading && filtered.length === 0 ? (
          <div className="empty">
            <RefreshCw size={20} style={{ animation: 'spin 1s linear infinite' }} />
            Calculando sinais...
          </div>
        ) : filtered.length === 0 ? (
          <div className="empty">
            <Minus size={24} color="var(--text-muted)" />
            Nenhum sinal com os filtros atuais.
          </div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 5 }}>
            {filtered.map((s, i) => <SignalRow key={s.id} s={s} index={i} />)}
          </div>
        )}
      </div>
    </div>
  );
}
