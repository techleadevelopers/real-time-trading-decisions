import {
  LayoutDashboard, TrendingUp, Globe, ShieldAlert,
  Layers, Activity, Server, Zap,
} from 'lucide-react';

interface SidebarProps { page: string; setPage: (p: string) => void; }

const NAV = [
  { id: 'dashboard', label: 'Painel Principal',    icon: LayoutDashboard, section: 'VISÃO GERAL' },
  { id: 'sinais',    label: 'Sinais de Trading',   icon: TrendingUp,      section: null },
  { id: 'regime',    label: 'Regime de Mercado',   icon: Globe,           section: 'ANÁLISE' },
  { id: 'risco',     label: 'Gestão de Risco',     icon: ShieldAlert,     section: null },
  { id: 'posicoes',  label: 'Posições',             icon: Layers,          section: 'EXECUÇÃO' },
  { id: 'execucao',  label: 'Ledger de Execução',  icon: Activity,        section: null },
  { id: 'saude',     label: 'Saúde do Sistema',    icon: Server,          section: 'SISTEMA' },
];

export default function Sidebar({ page, setPage }: SidebarProps) {
  return (
    <aside className="sidebar">
      <div className="sidebar-logo">
        <div className="logo-icon"><Zap size={16} /></div>
        <div>
          <div className="logo-text">Neural Edge</div>
          <div className="logo-sub">Scalp Bot · BingX</div>
        </div>
      </div>

      <nav className="sidebar-nav">
        {NAV.map((item, i) => (
          <div key={item.id}>
            {item.section && (
              <div className="nav-section" style={{ marginTop: i === 0 ? 4 : 10 }}>
                {item.section}
              </div>
            )}
            <div className={`nav-item${page === item.id ? ' active' : ''}`} onClick={() => setPage(item.id)}>
              <item.icon size={14} />
              {item.label}
              {item.id === 'dashboard' && (
                <div className="live-dot" style={{ marginLeft: 'auto', width: 5, height: 5 }} />
              )}
            </div>
          </div>
        ))}
      </nav>

      <div className="sidebar-footer">
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 6 }}>
          <div className="status-dot online" />
          <span style={{ fontSize: 10, color: 'var(--green)', fontFamily: 'var(--font-mono)' }}>DEMO ATIVO</span>
        </div>
        <div style={{ fontSize: 10, color: 'var(--text-muted)', fontFamily: 'var(--font-mono)' }}>v0.1.0 · BingX Futures</div>
      </div>
    </aside>
  );
}
