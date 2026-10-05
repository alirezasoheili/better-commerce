import { useEffect, useState } from 'react';
import { readJson } from '../../../shared/wire';

export default function Availability() {
  const [state, setState] = useState<'loading' | 'available' | 'unavailable'>('loading');
  async function refresh(signal?: AbortSignal) {
    setState('loading');
    try { await readJson<{ status: string }>('/healthz', undefined, signal); setState('available'); }
    catch { if (!signal?.aborted) setState('unavailable'); }
  }
  useEffect(() => {
    const controller = new AbortController();
    void refresh(controller.signal);
    return () => controller.abort();
  }, []);
  return <section className="availability" aria-label="Shop connection">
    <p role="status">{state === 'loading' ? 'Connecting to the shop…' : state === 'available' ? 'Connected to the shop' : 'The shop cannot be reached.'}</p>
    <button type="button" disabled={state === 'loading'} onClick={() => void refresh()}>Check again</button>
  </section>;
}
