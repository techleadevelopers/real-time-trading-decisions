import { Server, CheckCircle, Cpu, Code2, Zap, Activity, FlaskConical } from 'lucide-react';

function ServiceCard({
  name,
  description,
  port,
  tech,
  online,
  icon: Icon,
  color,
  details,
  metrics,
}: {
  name: string;
  description: string;
  port: string;
  tech: string;
  online: boolean;
  icon: React.ElementType;
  color: string;
  details: { label: string; value: string }[];
  metrics: { label: string; value: string; good?: boolean }[];
}) {
  return (
    <div
      className="card"
      style={{
        borderColor: 'rgba(16,185,129,0.2)',
        transition: 'border-color 0.3s',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'flex-start', gap: 14, marginBottom: 16 }}>
        <div style={{
          width: 44, height: 44, borderRadius: 10, display: 'flex',
          alignItems: 'center', justifyContent: 'center',
          background: `${color}20`, border: `1px solid ${color}40`, flexShrink: 0,
        }}>
          <Icon size={20} color={color} />
        </div>
        <div style={{ flex: 1 }}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8 }}>
            <div style={{ fontSize: 15, fontWeight: 700, color: 'var(--text-primary)' }}>{name}</div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <CheckCircle size={14} color="var(--green)" />
              <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--green)', fontFamily: 'var(--font-mono)' }}>online</span>
            </div>
          </div>
          <div style={{ fontSize: 12, color: 'var(--text-secondary)', marginTop: 2 }}>{description}</div>
        </div>
      </div>

      <div style={{ display: 'flex', gap: 8, marginBottom: 14, flexWrap: 'wrap' }}>
        <span className="badge badge-gray">porta {port}</span>
        <span className="badge" style={{ background: `${color}15`, color, border: `1px solid ${color}30` }}>{tech}</span>
        {online && <span className="badge badge-green">operacional</span>}
      </div>

      <hr className="sep" style={{ margin: '12px 0' }} />

      {details.map((d) => (
        <div className="metric-row" key={d.label}>
          <span className="metric-label">{d.label}</span>
          <span className="metric-value" style={{ fontSize: 12 }}>{d.value}</span>
        </div>
      ))}

      {metrics.length > 0 && (
        <>
          <div style={{ marginTop: 14, marginBottom: 8, fontSize: 10, fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.08em' }}>
            MÉTRICAS AO VIVO
          </div>
          {metrics.map((m) => (
            <div className="metric-row" key={m.label}>
              <span className="metric-label"><Activity size={10} style={{ marginRight: 4 }} />{m.label}</span>
              <span className="metric-value" style={{ fontSize: 12, color: m.good === false ? 'var(--red)' : m.good ? 'var(--green)' : 'var(--accent)' }}>
                {m.value}
              </span>
            </div>
          ))}
        </>
      )}
    </div>
  );
}

export default function SaudeSistema() {
  return (
    <div>
      {/* Demo notice */}
      <div style={{
        display: 'flex', alignItems: 'center', gap: 12, padding: '12px 18px',
        background: 'rgba(139,92,246,0.08)', border: '1px solid rgba(139,92,246,0.25)',
        borderRadius: 10, marginBottom: 20,
      }}>
        <FlaskConical size={16} color="var(--purple)" />
        <div>
          <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--purple)' }}>Modo Demo Ativo</div>
          <div style={{ fontSize: 12, color: 'var(--text-secondary)' }}>
            Os dados exibidos são simulados para demonstração. Em produção, cada serviço se conecta à exchange em tempo real.
          </div>
        </div>
      </div>

      {/* Overall status banner */}
      <div className="card" style={{
        marginBottom: 20,
        background: 'linear-gradient(135deg, rgba(16,185,129,0.08), rgba(14,165,233,0.05))',
        border: '1px solid rgba(16,185,129,0.2)',
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
          <Server size={24} color="var(--green)" />
          <div>
            <div style={{ fontSize: 16, fontWeight: 700, color: 'var(--text-primary)' }}>
              Sistema Totalmente Operacional
            </div>
            <div style={{ fontSize: 12, color: 'var(--text-secondary)' }}>
              Todos os 3 serviços ativos — Neural Edge Trading System
            </div>
          </div>
          <div style={{ marginLeft: 'auto', display: 'flex', gap: 10 }}>
            {['Python', 'Go', 'Rust'].map((s) => (
              <div key={s} style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
                <div className="status-dot online" />
                <span style={{ fontSize: 11, color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)' }}>{s}</span>
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* Service cards */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(340px, 1fr))', gap: 16 }}>
        <ServiceCard
          name="Backend Python"
          description="API FastAPI com análise de sinais, regime e modelo de ML"
          port="8000"
          tech="Python 3.12 / FastAPI"
          online={true}
          icon={Code2}
          color="var(--accent)"
          details={[
            { label: 'Framework', value: 'FastAPI 0.115' },
            { label: 'Modelo ML', value: 'Logistic Regression online' },
            { label: 'Dados', value: 'Binance WebSocket + REST' },
            { label: 'Indicadores', value: 'RSI, EMA, ATR, MACD, Vol-Z' },
            { label: 'Ativos monitorados', value: '10 pares USDT' },
          ]}
          metrics={[
            { label: 'Latência média', value: '48ms', good: true },
            { label: 'Sinais/hora', value: '~120', good: true },
            { label: 'Cache hit rate', value: '94%', good: true },
            { label: 'Uptime', value: '99.8%', good: true },
          ]}
        />

        <ServiceCard
          name="Plano de Controle (Go)"
          description="Gateway de execução, gestão de risco global e WebSocket hub"
          port="8088"
          tech="Go 1.21 / gorilla/websocket"
          online={true}
          icon={Zap}
          color="var(--purple)"
          details={[
            { label: 'Runtime', value: 'Go 1.21' },
            { label: 'Exchange', value: 'BingX / Binance Futures' },
            { label: 'WebSocket', value: '/ws — atualizações em tempo real' },
            { label: 'Kill Switch', value: 'Desativado' },
            { label: 'Reconciliação', value: 'Idempotência + ledger de fills' },
          ]}
          metrics={[
            { label: 'Latência de execução', value: '52ms', good: true },
            { label: 'Ordens fill/hour', value: '7', good: true },
            { label: 'Slippage médio', value: '-0.0012%', good: true },
            { label: 'WS clients', value: '1 conectado', good: true },
          ]}
        />

        <ServiceCard
          name="Motor RTTS (Rust)"
          description="Motor de decisão em tempo real com microestrutura e EV adaptativo"
          port="9898"
          tech="Rust / Tokio async"
          online={true}
          icon={Cpu}
          color="var(--yellow)"
          details={[
            { label: 'Runtime', value: 'Tokio async' },
            { label: 'Métricas', value: 'Prometheus (:9898/metrics)' },
            { label: 'Feeds', value: 'Binance L2 + AggTrade WebSocket' },
            { label: 'Estratégia', value: 'Microestrutura + EV adaptativo' },
            { label: 'Pipeline', value: 'Ingestion → Regime → Risk → Exec' },
          ]}
          metrics={[
            { label: 'Eventos processados', value: '28.4k/s', good: true },
            { label: 'Latência p99', value: '210µs', good: true },
            { label: 'Hit rate atual', value: '68.4%', good: true },
            { label: 'Ordens rejeitadas (risco)', value: '3', good: true },
          ]}
        />
      </div>

      {/* Architecture diagram */}
      <div className="card" style={{ marginTop: 16 }}>
        <div className="card-header">
          <span className="card-title">Arquitetura do Sistema</span>
        </div>
        <div style={{
          padding: '20px',
          background: 'var(--bg-input)',
          borderRadius: 8,
          border: '1px solid var(--border)',
          fontFamily: 'var(--font-mono)',
          fontSize: 12,
          color: 'var(--text-secondary)',
          lineHeight: 2,
          overflowX: 'auto',
        }}>
          <div style={{ color: 'var(--accent)', whiteSpace: 'pre' }}>{'┌─────────────────────────────────────────────────────────────────────┐'}</div>
          <div style={{ whiteSpace: 'pre' }}>{'│  '}<span style={{ color: 'var(--text-primary)' }}>{'Binance WebSocket (L2 + AggTrade)'}</span>{'                                │'}</div>
          <div style={{ whiteSpace: 'pre' }}>{'│           ↓                              ↓                          │'}</div>
          <div style={{ whiteSpace: 'pre' }}>{'│  '}<span style={{ color: 'var(--yellow)' }}>{'Motor Rust (RTTS :9898)'}</span>{'      '}<span style={{ color: 'var(--accent)' }}>{'Backend Python (:8000)'}</span>{'          │'}</div>
          <div style={{ whiteSpace: 'pre' }}>{'│           ↓ POST /execution/requests     ↓ sinais + regime         │'}</div>
          <div style={{ whiteSpace: 'pre' }}>{'│  '}<span style={{ color: 'var(--purple)' }}>{'Plano de Controle Go (:8088)'}</span>{'  ←── dados de análise              │'}</div>
          <div style={{ whiteSpace: 'pre' }}>{'│           ↓ REST + WebSocket                                        │'}</div>
          <div style={{ whiteSpace: 'pre' }}>{'│  '}<span style={{ color: 'var(--green)' }}>{'BingX / Binance Exchange (Produção)'}</span>{'                             │'}</div>
          <div style={{ whiteSpace: 'pre' }}>{'│           ↑ fills + confirmações                                    │'}</div>
          <div style={{ whiteSpace: 'pre' }}>{'│  '}<span style={{ color: 'var(--text-muted)' }}>{'Este Dashboard (React :5000) ← Python :8000 + Go :8088'}</span>{'          │'}</div>
          <div style={{ color: 'var(--accent)', whiteSpace: 'pre' }}>{'└─────────────────────────────────────────────────────────────────────┘'}</div>
        </div>
      </div>
    </div>
  );
}
