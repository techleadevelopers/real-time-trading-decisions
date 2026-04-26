import { ShieldAlert, Power, RefreshCw, TrendingDown, TrendingUp } from 'lucide-react';
import { usePolling } from '../hooks/useQuery';
import { fetchCPRisk, fetchCPAccount, postKillSwitch } from '../api/backend';
import { useState } from 'react';

interface RiskStatus {
  SystemStressIndex?: number;
  MempoolPressure?: number;
  ExecutionFragility?: number;
  ExposureRisk?: number;
  KillSwitch?: boolean;
  MarkoutDegradationScore?: number;
  AdverseSelectionEMA?: number;
}

interface AccountState {
  Balance?: number;
  Available?: number;
  UsedMargin?: number;
  Leverage?: number;
  UnrealizedPnl?: number;
}

function GaugeBar({ value, label }: { value: number; label: string }) {
  const pct = Math.max(0, Math.min(1, value));
  const color = pct > 0.7 ? 'var(--red)' : pct > 0.4 ? 'var(--yellow)' : 'var(--green)';
  const level = pct > 0.7 ? 'ALTO' : pct > 0.4 ? 'MÉDIO' : 'BAIXO';
  return (
    <div style={{ marginBottom: 18 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 6, alignItems: 'center' }}>
        <span style={{ fontSize: 12, color: 'var(--text-secondary)' }}>{label}</span>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 13, fontWeight: 700, color }}>{(pct * 100).toFixed(1)}%</span>
          <span className="badge" style={{ background: `${color}15`, color, border: `1px solid ${color}30`, padding: '1px 6px', fontSize: 10 }}>{level}</span>
        </div>
      </div>
      <div style={{ height: 6, background: 'var(--border)', borderRadius: 3, overflow: 'hidden' }}>
        <div style={{
          height: '100%', width: `${pct * 100}%`,
          background: `linear-gradient(90deg, ${color}88, ${color})`,
          borderRadius: 3, transition: 'width 0.6s ease',
        }} />
      </div>
    </div>
  );
}

function fmt(n: number | null | undefined, dec = 4) {
  if (n == null) return '—';
  return n.toFixed(dec);
}

function fmtUSD(n: number | null | undefined) {
  if (n == null) return '—';
  const prefix = n >= 0 ? '+' : '';
  return `${prefix}$${Math.abs(n).toLocaleString('pt-BR', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
}

export default function Risco() {
  const { data: risk, loading: riskLoading } = usePolling(fetchCPRisk, 5000);
  const { data: account, loading: accLoading } = usePolling(fetchCPAccount, 8000);
  const [ksEnabled, setKsEnabled] = useState(false);
  const [killPending, setKillPending] = useState(false);

  const r = risk as RiskStatus | null;
  const acc = account as AccountState | null;

  const handleKillSwitch = async () => {
    setKillPending(true);
    try {
      await postKillSwitch(!ksEnabled);
      setKsEnabled((v) => !v);
    } finally {
      setKillPending(false);
    }
  };

  const pnl = acc?.UnrealizedPnl ?? 0;
  const utilisacao = acc && acc.Balance ? (acc.UsedMargin ?? 0) / acc.Balance : 0;

  return (
    <div>
      {/* Kill switch */}
      <div className={`card${ksEnabled ? ' alert-card' : ''}`} style={{ marginBottom: 16 }}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: 12 }}>
          <div>
            <div className="card-title" style={{ marginBottom: 4 }}><Power size={13} />KILL SWITCH GLOBAL</div>
            <div style={{ fontSize: 12, color: 'var(--text-secondary)' }}>
              Quando ativado, para toda execução de ordens no plano de controle.
            </div>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 14, fontWeight: 700, color: ksEnabled ? 'var(--red)' : 'var(--green)' }}>
              {ksEnabled ? '⛔  ATIVADO' : '✅  DESATIVADO'}
            </span>
            <button
              className={ksEnabled ? 'btn btn-ghost' : 'btn btn-danger'}
              onClick={handleKillSwitch}
              disabled={killPending}
            >
              <Power size={12} />
              {killPending ? 'Aguardando...' : ksEnabled ? 'Desativar' : 'Ativar Kill Switch'}
            </button>
          </div>
        </div>
      </div>

      <div className="grid-2">
        {/* Risk gauges */}
        <div className="card">
          <div className="card-header">
            <span className="card-title"><ShieldAlert size={13} />Índices de Risco em Tempo Real</span>
            {riskLoading && <RefreshCw size={12} style={{ animation: 'spin 1s linear infinite', color: 'var(--text-muted)' }} />}
          </div>
          {r ? (
            <>
              <GaugeBar value={r.SystemStressIndex ?? 0} label="Estresse do Sistema" />
              <GaugeBar value={r.MempoolPressure ?? 0} label="Pressão Mempool" />
              <GaugeBar value={r.ExecutionFragility ?? 0} label="Fragilidade de Execução" />
              <GaugeBar value={r.ExposureRisk ?? 0} label="Risco de Exposição" />
              <hr className="sep" />
              <div className="metric-row">
                <span className="metric-label">Degradação de Markout</span>
                <span className="metric-value">{fmt(r.MarkoutDegradationScore)}</span>
              </div>
              <div className="metric-row">
                <span className="metric-label">Seleção Adversa (EMA)</span>
                <span className="metric-value">{fmt(r.AdverseSelectionEMA)}</span>
              </div>
            </>
          ) : (
            <div className="empty"><RefreshCw size={18} style={{ animation: 'spin 1s linear infinite' }} />Carregando...</div>
          )}
        </div>

        {/* Account state */}
        <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
          <div className="card">
            <div className="card-header">
              <span className="card-title">Estado da Conta</span>
              {accLoading && <RefreshCw size={12} style={{ animation: 'spin 1s linear infinite', color: 'var(--text-muted)' }} />}
            </div>
            {acc ? (
              <>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 14, marginBottom: 16 }}>
                  <div style={{ background: 'var(--bg-input)', borderRadius: 8, padding: 14, border: '1px solid var(--border)' }}>
                    <div style={{ fontSize: 11, color: 'var(--text-secondary)', marginBottom: 4 }}>Saldo Total</div>
                    <div style={{ fontFamily: 'var(--font-mono)', fontSize: 18, fontWeight: 700, color: 'var(--green)' }}>
                      ${acc.Balance?.toLocaleString('pt-BR', { minimumFractionDigits: 2 })}
                    </div>
                  </div>
                  <div style={{ background: 'var(--bg-input)', borderRadius: 8, padding: 14, border: '1px solid var(--border)' }}>
                    <div style={{ fontSize: 11, color: 'var(--text-secondary)', marginBottom: 4 }}>PnL Não Realizado</div>
                    <div style={{ fontFamily: 'var(--font-mono)', fontSize: 18, fontWeight: 700, color: pnl >= 0 ? 'var(--green)' : 'var(--red)' }}>
                      {pnl >= 0 ? <TrendingUp size={14} style={{ display: 'inline', marginRight: 4 }} /> : <TrendingDown size={14} style={{ display: 'inline', marginRight: 4 }} />}
                      {fmtUSD(pnl)}
                    </div>
                  </div>
                </div>
                <div className="metric-row">
                  <span className="metric-label">Disponível</span>
                  <span className="metric-value">${acc.Available?.toLocaleString('pt-BR', { minimumFractionDigits: 2 })}</span>
                </div>
                <div className="metric-row">
                  <span className="metric-label">Margem em Uso</span>
                  <span className="metric-value num-yellow">${acc.UsedMargin?.toLocaleString('pt-BR', { minimumFractionDigits: 2 })}</span>
                </div>
                <div className="metric-row">
                  <span className="metric-label">Alavancagem</span>
                  <span className="metric-value num-accent">{acc.Leverage}x</span>
                </div>
                <div style={{ marginTop: 12 }}>
                  <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 4, fontSize: 12, color: 'var(--text-secondary)' }}>
                    <span>Utilização de Margem</span>
                    <span style={{ fontFamily: 'var(--font-mono)', color: utilisacao > 0.7 ? 'var(--red)' : 'var(--accent)' }}>{(utilisacao * 100).toFixed(1)}%</span>
                  </div>
                  <div style={{ height: 4, background: 'var(--border)', borderRadius: 2, overflow: 'hidden' }}>
                    <div style={{ height: '100%', width: `${utilisacao * 100}%`, background: utilisacao > 0.7 ? 'var(--red)' : 'var(--accent)', borderRadius: 2, transition: 'width 0.6s' }} />
                  </div>
                </div>
              </>
            ) : (
              <div className="empty"><RefreshCw size={18} style={{ animation: 'spin 1s linear infinite' }} />Carregando...</div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
