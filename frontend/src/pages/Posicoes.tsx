import { Layers, RefreshCw, TrendingUp, TrendingDown } from 'lucide-react';
import { usePolling } from '../hooks/useQuery';
import { fetchCPPositions, fetchCPAccount } from '../api/backend';
import { DEMO_SIGNALS } from '../api/demo';

interface Position {
  Symbol: string;
  Size: number;
  AvgPrice: number;
  Updated: string;
}

interface AccountState {
  Balance?: number;
  Available?: number;
  UsedMargin?: number;
  UnrealizedPnl?: number;
}

function fmt(n: number | null | undefined, dec = 4) {
  if (n == null) return '—';
  return n.toFixed(dec);
}

export default function Posicoes() {
  const { data, loading } = usePolling(fetchCPPositions, 5000);
  const { data: account } = usePolling(fetchCPAccount, 8000);
  const positions = (data as Position[] | null) || [];
  const acc = account as AccountState | null;

  // Calculate approximate current prices from signals for PnL
  const getCurrentPrice = (symbol: string) => {
    const sig = DEMO_SIGNALS.find((s) => s.symbol === symbol);
    return sig?.entry_price ?? null;
  };

  const calcPnl = (pos: Position) => {
    const curPrice = getCurrentPrice(pos.Symbol);
    if (!curPrice) return null;
    const diff = (curPrice - pos.AvgPrice) * pos.Size;
    return diff;
  };

  const totalPnl = positions.reduce((sum, p) => {
    const pnl = calcPnl(p);
    return sum + (pnl ?? 0);
  }, 0);

  return (
    <div>
      {/* KPI row */}
      <div className="grid-4" style={{ marginBottom: 16 }}>
        <div className="card">
          <div className="card-title" style={{ marginBottom: 8 }}><Layers size={13} />Posições Abertas</div>
          <div className="card-value">{loading ? '—' : positions.length}</div>
        </div>
        <div className="card">
          <div className="card-title" style={{ marginBottom: 8 }}>Posições LONG</div>
          <div className="card-value num-green">{loading ? '—' : positions.filter((p) => p.Size > 0).length}</div>
        </div>
        <div className="card">
          <div className="card-title" style={{ marginBottom: 8 }}>Posições SHORT</div>
          <div className="card-value num-red">{loading ? '—' : positions.filter((p) => p.Size < 0).length}</div>
        </div>
        <div className="card">
          <div className="card-title" style={{ marginBottom: 8 }}>PnL Estimado</div>
          <div className="card-value" style={{ color: totalPnl >= 0 ? 'var(--green)' : 'var(--red)' }}>
            {totalPnl >= 0 ? '+' : ''}${Math.abs(totalPnl).toFixed(2)}
          </div>
        </div>
      </div>

      <div className="card">
        <div className="card-header">
          <span className="card-title"><Layers size={13} />Posições Ativas</span>
          {loading && <RefreshCw size={12} style={{ animation: 'spin 1s linear infinite', color: 'var(--text-muted)' }} />}
        </div>

        {loading && positions.length === 0 ? (
          <div className="empty"><RefreshCw size={22} style={{ animation: 'spin 1s linear infinite' }} />Carregando posições...</div>
        ) : positions.length === 0 ? (
          <div className="empty"><Layers size={28} color="var(--text-muted)" />Nenhuma posição aberta.</div>
        ) : (
          <div className="table-wrapper">
            <table>
              <thead>
                <tr>
                  <th>Ativo</th>
                  <th>Direção</th>
                  <th>Tamanho</th>
                  <th>Preço Médio</th>
                  <th>Preço Atual</th>
                  <th>PnL Estimado</th>
                  <th>Abertura</th>
                </tr>
              </thead>
              <tbody>
                {positions.map((p, i) => {
                  const curPrice = getCurrentPrice(p.Symbol);
                  const pnl = calcPnl(p);
                  const long = p.Size > 0;
                  return (
                    <tr key={i}>
                      <td><span style={{ fontFamily: 'var(--font-mono)', fontWeight: 600 }}>{p.Symbol}</span></td>
                      <td>
                        {long
                          ? <span className="badge badge-green"><TrendingUp size={10} />LONG</span>
                          : <span className="badge badge-red"><TrendingDown size={10} />SHORT</span>}
                      </td>
                      <td className="td-mono" style={{ color: long ? 'var(--green)' : 'var(--red)' }}>{Math.abs(p.Size).toFixed(4)}</td>
                      <td className="td-mono">{fmt(p.AvgPrice)}</td>
                      <td className="td-mono num-accent">{curPrice ? fmt(curPrice) : '—'}</td>
                      <td className="td-mono" style={{ color: pnl != null ? (pnl >= 0 ? 'var(--green)' : 'var(--red)') : 'var(--text-muted)', fontWeight: 600 }}>
                        {pnl != null ? `${pnl >= 0 ? '+' : ''}$${Math.abs(pnl).toFixed(2)}` : '—'}
                      </td>
                      <td className="td-mono" style={{ color: 'var(--text-secondary)' }}>
                        {p.Updated ? new Date(p.Updated).toLocaleString('pt-BR') : '—'}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </div>

      {/* Account summary */}
      {acc && (
        <div className="card" style={{ marginTop: 16 }}>
          <div className="card-header">
            <span className="card-title">Resumo da Conta</span>
          </div>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(160px, 1fr))', gap: 12 }}>
            {[
              { label: 'Saldo Total', value: `$${acc.Balance?.toLocaleString('pt-BR', { minimumFractionDigits: 2 })}`, color: 'var(--green)' },
              { label: 'Disponível', value: `$${acc.Available?.toLocaleString('pt-BR', { minimumFractionDigits: 2 })}`, color: 'var(--text-primary)' },
              { label: 'Margem em Uso', value: `$${acc.UsedMargin?.toLocaleString('pt-BR', { minimumFractionDigits: 2 })}`, color: 'var(--yellow)' },
              { label: 'PnL Não Realizado', value: `${(acc.UnrealizedPnl ?? 0) >= 0 ? '+' : ''}$${Math.abs(acc.UnrealizedPnl ?? 0).toFixed(2)}`, color: (acc.UnrealizedPnl ?? 0) >= 0 ? 'var(--green)' : 'var(--red)' },
            ].map((item) => (
              <div key={item.label} style={{ background: 'var(--bg-input)', borderRadius: 8, padding: '12px 14px', border: '1px solid var(--border)' }}>
                <div style={{ fontSize: 11, color: 'var(--text-secondary)', marginBottom: 4 }}>{item.label}</div>
                <div style={{ fontFamily: 'var(--font-mono)', fontSize: 15, fontWeight: 700, color: item.color }}>{item.value}</div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
