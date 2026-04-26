import { useState, useEffect, useCallback, useRef } from 'react';

export function usePolling<T>(
  fetcher: () => Promise<T>,
  intervalMs = 5000,
  enabled = true
) {
  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const run = useCallback(async () => {
    try {
      const result = await fetcher();
      setData(result);
      setError(null);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Erro desconhecido');
    } finally {
      setLoading(false);
    }
  }, [fetcher]);

  useEffect(() => {
    if (!enabled) return;
    run();
    timerRef.current = setInterval(run, intervalMs);
    return () => { if (timerRef.current) clearInterval(timerRef.current); };
  }, [run, intervalMs, enabled]);

  return { data, loading, error, refetch: run };
}

export function useServiceHealth(fetcher: () => Promise<boolean>, intervalMs = 8000) {
  const [online, setOnline] = useState<boolean | null>(null);

  useEffect(() => {
    const check = async () => {
      try {
        const ok = await fetcher();
        setOnline(ok);
      } catch {
        setOnline(false);
      }
    };
    check();
    const timer = setInterval(check, intervalMs);
    return () => clearInterval(timer);
  }, [fetcher, intervalMs]);

  return online;
}

export function useCPWebSocket(onMessage: (msg: unknown) => void) {
  const [connected, setConnected] = useState(false);

  // Demo: simulate connection after 1s
  useEffect(() => {
    const t = setTimeout(() => setConnected(true), 1000);
    return () => clearTimeout(t);
  }, [onMessage]);

  return connected;
}
