import { Globe, RefreshCw, TrendingUp, TrendingDown } from 'lucide-react';
import { usePolling } from '../hooks/useQuery';
import { fetchMetrics } from '../api/backend';

interface MetricEntry {
  hit_rate: number | null;
  count: number;
}

interface Metrics {
  symbols: Record<string, MetricEntry>;
  regimes: Record<string, MetricEntry>;
}

function pct(n: number | null | undefined) {
  if (n == null) return '—';
  return `${(n * 100).toFixed(1)}%`;
}

function RegimeCard({ name }: { name: string }) {
  const colors: Record<string, { bg: string; border: string; color: string; label: string }> = {
    BULL: { bg: 'var(--green-glow)', border: 'rgba(16,185,129,0.3)', color: 'var(--green)', label: 'Alta' },
    BEAR: { bg: 'var(--red-glow)', border: 'rgba(239,68,68,0.3)', color: 'var(--red)', label: 'Baixa' },
    CHOP: { bg: 'rgba(100,116,139,0.1)', border: 'var(--border)', color: 'var(--text-secondary)', label: 'Lateral' },
    ALT: { bg: 'rgba(139,92,246,0.1)', border: 'rgba(139,92,246,0.3)', color: 'var(--purple)', label: 'Altseason' },
  };
  const c = colors[name] || colors.CHOP;
  return (
    <span
      className="regime-badge"
      style={{ background: c.bg, border: `1px solid ${c.border}`, color: c.color }}
    >
      {name === 'BULL' && <TrendingUp size={15} />}
      {name === 'BEAR' && <TrendingDown size={15} />}
      {name} — {c.label}
    </span>
  );
}

export default function Regime() {
  const { data, loading, error } = usePolling(fetchMetrics, 30000);
  const metrics = data as Metrics | null;

  const regimes = Object.entries(metrics?.regimes || {});
  const symbols = Object.entries(metrics?.symbols || {}).sort((a, b) => (b[1].count || 0) - (a[1].count || 0));

  return (
    <div>
      {error && (
        <div style={{ padding: '14px 18px', background: 'var(--red-glow)', border: '1px solid rgba(239,68,68,0.3)', borderRadius: 8, marginBottom: 16, fontSize: 13, color: 'var(--red)' }}>
          {error}
        </div>
      )}

      {loading ? (
        <div className="empty">
          <RefreshCw size={22} style={{ animation: 'spin 1s linear infinite' }} />
          Carregando dados de regime...
        </div>
      ) : (
        <>
          {/* Regime performance grid */}
          <div className="card" style={{ marginBottom: 16 }}>
            <div className="card-header">
              <span className="card-title"><Globe size={13} />Performance por Regime</span>
            </div>
            {regimes.length === 0 ? (
              <div className="empty" style={{ padding: '24px 0' }}>Nenhum dado de regime disponível ainda.</div>
            ) : (
              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(200px, 1fr))', gap: 14 }}>
                {regimes.map(([regime, m]) => (
                  <div
                    key={regime}
                    style={{
                      background: 'var(--bg-input)',
                      border: '1px solid var(--border)',
                      borderRadius: 10,
                      padding: '16px',
                    }}
                  >
                    <div style={{ marginBottom: 12 }}>
                      <RegimeCard name={regime} />
                    </div>
                    <div className="metric-row">
                      <span className="metric-label">Operações</span>
                      <span className="metric-value">{m.count}</span>
                    </div>
                    <div className="metric-row">
                      <span className="metric-label">Taxa de Acerto</span>
                      <span className="metric-value" style={{ color: m.hit_rate != null && m.hit_rate >= 0.5 ? 'var(--green)' : 'var(--red)' }}>
                        {pct(m.hit_rate)}
                      </span>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>

          {/* Symbol performance table */}
          <div className="card">
            <div className="card-header">
              <span className="card-title">Performance por Ativo</span>
              <span style={{ fontSize: 11, color: 'var(--text-muted)', fontFamily: 'var(--font-mono)' }}>{symbols.length} ativos</span>
            </div>
            {symbols.length === 0 ? (
              <div className="empty" style={{ padding: '24px 0' }}>Nenhum dado disponível ainda.</div>
            ) : (
              <div className="table-wrapper">
                <table>
                  <thead>
                    <tr>
                      <th>Ativo</th>
                      <th>Total Sinais</th>
                      <th>Taxa de Acerto</th>
                      <th>Avaliação</th>
                    </tr>
                  </thead>
                  <tbody>
                    {symbols.map(([symbol, m]) => {
                      const hr = m.hit_rate ?? 0;
                      const rating = hr >= 0.7 ? { label: 'Excelente', color: 'var(--green)' }
                        : hr >= 0.55 ? { label: 'Bom', color: 'var(--accent)' }
                        : hr >= 0.45 ? { label: 'Neutro', color: 'var(--text-secondary)' }
                        : { label: 'Ruim', color: 'var(--red)' };
                      return (
                        <tr key={symbol}>
                          <td><span style={{ fontFamily: 'var(--font-mono)', fontWeight: 600 }}>{symbol}</span></td>
                          <td className="td-mono">{m.count}</td>
                          <td className="td-mono" style={{ color: hr >= 0.5 ? 'var(--green)' : 'var(--red)' }}>{pct(m.hit_rate)}</td>
                          <td><span className="badge" style={{ background: 'transparent', color: rating.color, border: `1px solid ${rating.color}33`, padding: '2px 8px' }}>{rating.label}</span></td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
}
