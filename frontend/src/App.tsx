import { useState, useCallback } from 'react';
import NeonBackground from './components/NeonBackground';
import Sidebar from './components/Sidebar';
import Topbar from './components/Topbar';
import Dashboard from './pages/Dashboard';
import Sinais from './pages/Sinais';
import Regime from './pages/Regime';
import Risco from './pages/Risco';
import Posicoes from './pages/Posicoes';
import Execucao from './pages/Execucao';
import SaudeSistema from './pages/SaudeSistema';
import { useServiceHealth, useCPWebSocket } from './hooks/useQuery';
import { fetchHealth, fetchCPHealth } from './api/backend';

export default function App() {
  const [page, setPage] = useState('dashboard');

  const pyOnline = useServiceHealth(fetchHealth, 8000);
  const cpOnline = useServiceHealth(fetchCPHealth, 8000);

  const handleWsMessage = useCallback((_msg: unknown) => {}, []);
  const wsConnected = useCPWebSocket(handleWsMessage);

  const renderPage = () => {
    switch (page) {
      case 'dashboard': return <Dashboard />;
      case 'sinais':    return <Sinais />;
      case 'regime':    return <Regime />;
      case 'risco':     return <Risco />;
      case 'posicoes':  return <Posicoes />;
      case 'execucao':  return <Execucao />;
      case 'saude':     return <SaudeSistema />;
      default:          return <Dashboard />;
    }
  };

  return (
    <>
      <NeonBackground />
      <div className="layout" style={{ position: 'relative', zIndex: 1 }}>
        <Sidebar page={page} setPage={setPage} />
        <div className="main-content">
          <Topbar page={page} pyOnline={pyOnline} cpOnline={cpOnline} wsConnected={wsConnected} />
          <div className="page-body">
            {renderPage()}
          </div>
        </div>
      </div>
    </>
  );
}
