import { RefreshCw, FlaskConical } from 'lucide-react';

const PAGE_TITLES: Record<string, { title: string; sub: string }> = {
  dashboard: { title: 'Painel Principal', sub: 'Visão consolidada do sistema' },
  sinais: { title: 'Sinais de Trading', sub: 'Análise em tempo real de múltiplos ativos' },
  regime: { title: 'Regime de Mercado', sub: 'Classificação macro do mercado cripto' },
  risco: { title: 'Gestão de Risco', sub: 'Métricas de risco e controles do plano de controle' },
  posicoes: { title: 'Posições Abertas', sub: 'Posições ativas e PnL não realizado' },
  execucao: { title: 'Ledger de Execução', sub: 'Histórico de fills e eventos de execução' },
  saude: { title: 'Saúde do Sistema', sub: 'Status dos serviços Python, Go e Rust' },
};

interface TopbarProps {
  page: string;
  pyOnline: boolean | null;
  cpOnline: boolean | null;
  wsConnected: boolean;
}

export default function Topbar({ page, pyOnline, cpOnline, wsConnected }: TopbarProps) {
  const info = PAGE_TITLES[page] || PAGE_TITLES.dashboard;

  return (
    <header className="topbar">
      <div className="topbar-left">
        <div>
          <div className="page-title">{info.title}</div>
          <div className="page-sub">{info.sub}</div>
        </div>
      </div>
      <div className="topbar-right">
        {/* Demo badge */}
        <div style={{
          display: 'flex', alignItems: 'center', gap: 6,
          padding: '4px 10px',
          background: 'rgba(139,92,246,0.12)',
          border: '1px solid rgba(139,92,246,0.35)',
          borderRadius: 20,
        }}>
          <FlaskConical size={11} color="var(--purple)" />
          <span style={{ fontSize: 11, fontWeight: 600, color: 'var(--purple)', fontFamily: 'var(--font-mono)', letterSpacing: '0.06em' }}>MODO DEMO</span>
        </div>

        {/* Service indicators */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 14, marginLeft: 8 }}>
          <ServicePill label="Python API" online={pyOnline} />
          <ServicePill label="Go CP" online={cpOnline} />
          <WsPill connected={wsConnected} />
        </div>
      </div>
    </header>
  );
}

function ServicePill({ label, online }: { label: string; online: boolean | null }) {
  const color = online === null ? 'var(--text-muted)' : online ? 'var(--green)' : 'var(--red)';
  const text = online === null ? '...' : online ? 'online' : 'offline';
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
      <div
        className="status-dot"
        style={{
          background: color,
          boxShadow: online ? `0 0 6px ${color}` : 'none',
          animation: online ? 'pulse-green 2s infinite' : 'none',
          width: 7, height: 7,
        }}
      />
      <span style={{ fontSize: 11, color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)' }}>
        {label}&nbsp;<span style={{ color }}>{text}</span>
      </span>
    </div>
  );
}

function WsPill({ connected }: { connected: boolean }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
      <RefreshCw
        size={11}
        color={connected ? 'var(--green)' : 'var(--text-muted)'}
        style={{ animation: connected ? 'spin 3s linear infinite' : 'none' }}
      />
      <span style={{ fontSize: 11, color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)' }}>
        WS&nbsp;<span style={{ color: connected ? 'var(--green)' : 'var(--text-muted)' }}>
          {connected ? 'ativo' : 'aguardando'}
        </span>
      </span>
    </div>
  );
}
