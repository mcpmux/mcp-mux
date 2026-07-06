import { useEffect, useState } from 'react';
import { isWebAdmin, WEB_ADMIN_TOKEN_KEY } from '@/lib/transport';

/**
 * Auth gate for the browser web admin (the desktop React app served headless by
 * `mcpmux serve`). The Tauri shell trusts IPC, so this is a passthrough there.
 * In the browser it requires the admin token once (validated against the
 * management API), stores it for the HTTP transport, then renders the app.
 */
export function WebAdminGate({ children }: { children: React.ReactNode }) {
  // Desktop (Tauri) — no gate.
  if (!isWebAdmin) return <>{children}</>;
  return <WebLogin>{children}</WebLogin>;
}

async function validateToken(token: string): Promise<boolean> {
  try {
    const res = await fetch('/admin/api/info', {
      headers: { Authorization: `Bearer ${token}` },
    });
    return res.ok;
  } catch {
    return false;
  }
}

function WebLogin({ children }: { children: React.ReactNode }) {
  const [authed, setAuthed] = useState<boolean | null>(null);
  const [token, setToken] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  // On mount, accept an already-stored token if it still validates. The
  // resolution is always async (a microtask at minimum), so it doesn't trigger
  // the synchronous-setState-in-effect cascade the lint guards against.
  useEffect(() => {
    let cancelled = false;
    const stored = localStorage.getItem(WEB_ADMIN_TOKEN_KEY);
    const resolve = stored ? validateToken(stored) : Promise.resolve(false);
    void resolve.then((ok) => {
      if (cancelled) return;
      if (!ok) localStorage.removeItem(WEB_ADMIN_TOKEN_KEY);
      setAuthed(ok);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const signIn = async () => {
    const t = token.trim();
    if (!t) {
      setError('Enter the admin token.');
      return;
    }
    setBusy(true);
    setError('');
    const ok = await validateToken(t);
    setBusy(false);
    if (ok) {
      localStorage.setItem(WEB_ADMIN_TOKEN_KEY, t);
      setAuthed(true);
    } else {
      setError('Invalid token.');
    }
  };

  if (authed === null) {
    return (
      <div style={centered}>
        <span style={{ color: '#9a9aa5' }}>Loading…</span>
      </div>
    );
  }
  if (authed) return <>{children}</>;

  return (
    <div style={centered}>
      <div style={card}>
        <h1 style={{ fontSize: 20, margin: '0 0 4px' }}>McpMux</h1>
        <p style={{ color: '#9a9aa5', fontSize: 14, margin: '0 0 20px' }}>
          Sign in to the web admin. Paste the admin token printed by <code>mcpmux serve</code> (or
          set via <code>MCPMUX_ADMIN_TOKEN</code>).
        </p>
        <input
          type="password"
          value={token}
          onChange={(e) => setToken(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') void signIn();
          }}
          placeholder="admin token"
          autoComplete="off"
          style={input}
          data-testid="web-admin-token"
        />
        <button onClick={() => void signIn()} disabled={busy} style={button} data-testid="web-admin-signin">
          {busy ? 'Checking…' : 'Sign in'}
        </button>
        {error && (
          <p style={{ color: '#f87171', fontSize: 13, marginTop: 10 }} data-testid="web-admin-error">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}

const centered: React.CSSProperties = {
  minHeight: '100vh',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  background: '#0b0b0f',
  color: '#e7e7ea',
  fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif',
  padding: 20,
};
const card: React.CSSProperties = {
  width: '100%',
  maxWidth: 420,
  background: '#16161c',
  border: '1px solid #2a2a33',
  borderRadius: 16,
  padding: 24,
};
const input: React.CSSProperties = {
  width: '100%',
  padding: '10px 12px',
  borderRadius: 10,
  border: '1px solid #2a2a33',
  background: '#0f0f14',
  color: 'inherit',
  fontSize: 14,
  marginBottom: 12,
};
const button: React.CSSProperties = {
  width: '100%',
  padding: '11px 16px',
  border: 'none',
  borderRadius: 10,
  background: '#6d5efc',
  color: '#fff',
  fontSize: 15,
  fontWeight: 600,
  cursor: 'pointer',
};
