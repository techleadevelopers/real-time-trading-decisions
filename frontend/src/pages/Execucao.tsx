import { Activity, RefreshCw, AlertTriangle } from 'lucide-react';
import { usePolling } from '../hooks/useQuery';
import { fetchCPLedger, fetchCPEvents, fetchCPReconciliation } from '../api/backend';
import { useState } from 'react';

interface LedgerEntry {
  OrderID?: string;
  Symbol?: string;
  Side?: string;
  FilledQty?: number;
  Price?: number;
  Timestamp?: string;
  Fee?: number;
}

interface ExecEvent {
  OrderID?: string;
  Symbol?: string;
  Slippage?: number;
  LatencyMs?: number;
  Regime?: string;
}

function fmt(n: number | null | undefined, dec = 4) {
  if (n == null) return '—';
  return n.toFixed(dec);
}

export default function Execucao() {
  const [tab, setTab] = useState<'ledger' | 'eventos' | 'reconciliacao'>('ledger');
  const { data: ledger, loading: lLoad, error: lError } = usePolling(fetchCPLedger, 8000);
  const { data: events, loading: eLoad, error: eError } = usePolling(fetchCPEvents, 8000);
  const { data: recon, loading: rLoad } = usePolling(fetchCPReconciliation, 15000);

  const ledgerList = (ledger as LedgerEntry[] | null) || [];
  const eventList = (events as ExecEvent[] | null) || [];

  const cpUnavailable = lError && eError;

  if (cpUnavailable) {
    return (
      <div style={{ padding: '20px', background: 'var(--red-glow)', border: '1px solid rgba(239,68,68,0.3)', borderRadius: 12, color: 'var(--red)', fontSize: 13 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <AlertTriangle size={15} />
          <strong>Plano de Controle Indisponível</strong>
        </div>
        <div style={{ marginTop: 6 }}>Serviço Go não acessível na porta 8088.</div>
      </div>
    );
  }

  return (
    <div>
      <div className="tabs" style={{ marginBottom: 16, maxWidth: 360 }}>
        {([['ledger', 'Ledger de Fills'], ['eventos', 'Eventos de Execução'], ['reconciliacao', 'Reconciliação']] as const).map(([id, label]) => (
          <button key={id} className={`tab-btn${tab === id ? ' active' : ''}`} onClick={() => setTab(id)}>
            {label}
          </button>
        ))}
      </div>

      {tab === 'ledger' && (
        <div className="card">
          <div className="card-header">
            <span className="card-title"><Activity size={13} />Ledger de Fills</span>
            {lLoad && <RefreshCw size={12} style={{ animation: 'spin 1s linear infinite', color: 'var(--text-muted)' }} />}
          </div>
          {lLoad && ledgerList.length === 0 ? (
            <div className="empty"><RefreshCw size={20} style={{ animation: 'spin 1s linear infinite' }} />Carregando...</div>
          ) : ledgerList.length === 0 ? (
            <div className="empty">Nenhum fill registrado.</div>
          ) : (
            <div className="table-wrapper">
              <table>
                <thead>
                  <tr>
                    <th>ID da Ordem</th>
                    <th>Ativo</th>
                    <th>Lado</th>
                    <th>Qtd Executada</th>
                    <th>Preço</th>
                    <th>Taxa</th>
                    <th>Horário</th>
                  </tr>
                </thead>
                <tbody>
                  {ledgerList.slice(-50).reverse().map((e, i) => (
                    <tr key={i}>
                      <td className="td-mono" style={{ color: 'var(--text-secondary)', fontSize: 10 }}>{e.OrderID?.slice(0, 12) || '—'}...</td>
                      <td><span style={{ fontFamily: 'var(--font-mono)', fontWeight: 600 }}>{e.Symbol || '—'}</span></td>
                      <td>{e.Side === 'BUY' ? <span className="badge badge-green">COMPRA</span> : <span className="badge badge-red">VENDA</span>}</td>
                      <td className="td-mono">{fmt(e.FilledQty)}</td>
                      <td className="td-mono">{fmt(e.Price)}</td>
                      <td className="td-mono" style={{ color: 'var(--red)' }}>{fmt(e.Fee, 6)}</td>
                      <td className="td-mono" style={{ color: 'var(--text-secondary)' }}>
                        {e.Timestamp ? new Date(e.Timestamp).toLocaleString('pt-BR') : '—'}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      {tab === 'eventos' && (
        <div className="card">
          <div className="card-header">
            <span className="card-title">Eventos de Execução</span>
            {eLoad && <RefreshCw size={12} style={{ animation: 'spin 1s linear infinite', color: 'var(--text-muted)' }} />}
          </div>
          {eLoad && eventList.length === 0 ? (
            <div className="empty"><RefreshCw size={20} style={{ animation: 'spin 1s linear infinite' }} />Carregando...</div>
          ) : eventList.length === 0 ? (
            <div className="empty">Nenhum evento registrado.</div>
          ) : (
            <div className="table-wrapper">
              <table>
                <thead>
                  <tr>
                    <th>Ordem</th>
                    <th>Ativo</th>
                    <th>Slippage</th>
                    <th>Latência</th>
                    <th>Regime</th>
                  </tr>
                </thead>
                <tbody>
                  {eventList.slice(-50).reverse().map((e, i) => (
                    <tr key={i}>
                      <td className="td-mono" style={{ color: 'var(--text-secondary)', fontSize: 10 }}>{e.OrderID?.slice(0, 12) || '—'}...</td>
                      <td><span style={{ fontFamily: 'var(--font-mono)', fontWeight: 600 }}>{e.Symbol || '—'}</span></td>
                      <td className="td-mono" style={{ color: e.Slippage != null && e.Slippage < 0 ? 'var(--red)' : 'var(--green)' }}>{fmt(e.Slippage, 6)}</td>
                      <td className="td-mono">{e.LatencyMs != null ? `${e.LatencyMs}ms` : '—'}</td>
                      <td><span className="badge badge-purple">{e.Regime || '—'}</span></td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      {tab === 'reconciliacao' && (
        <div className="card">
          <div className="card-header">
            <span className="card-title">Reconciliação de Ordens</span>
            {rLoad && <RefreshCw size={12} style={{ animation: 'spin 1s linear infinite', color: 'var(--text-muted)' }} />}
          </div>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--text-secondary)', padding: '12px 0' }}>
            {rLoad ? 'Carregando...' : (
              recon
                ? <pre style={{ whiteSpace: 'pre-wrap', color: 'var(--text-primary)' }}>{JSON.stringify(recon, null, 2)}</pre>
                : 'Nenhum dado de reconciliação disponível.'
            )}
          </div>
        </div>
      )}
    </div>
  );
}
