import React, { useState, useEffect, useCallback } from 'react';
import './App.css';

const API = '';  // proxy via package.json
const ROLES = ['data_entry', 'kyc_officer', 'document_officer', 'underwriter', 'senior_underwriter', 'branch_manager', 'collections', 'customer'];

// ── Types ──────────────────────────────────────────────────────────────

interface Projection {
  resource: {
    id: string; resource_type: string; state: string;
    desired_state: string | null; data: Record<string, any>;
    version: number; is_terminal: boolean;
  };
  valid_actions: { action: string; action_type: string }[];
  blocked_actions: { action: string; reason: string; blocked_by: string }[];
  warnings: { message: string; source: string; severity: string }[];
  proposals: { agent: string; action: string; confidence: number; reasoning: string }[];
  audit_summary: { actor: string; from_state: string; to_state: string; authority_level: string; timestamp: string }[];
}

// ── App ────────────────────────────────────────────────────────────────

function App() {
  const [role, setRole] = useState('manager');
  const [loans, setLoans] = useState<any[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [tick, setTick] = useState(0);

  const refresh = () => setTick(t => t + 1);

  useEffect(() => {
    fetch(`${API}/api/loan`).then(r => r.json()).then(setLoans).catch(() => {});
  }, [tick]);

  return (
    <div className="app">
      <header className="header">
        <h1>Reconcile</h1>
        <span className="subtitle">Loan Operating System</span>
        <div className="role-picker">
          <label>Role:</label>
          <select value={role} onChange={e => setRole(e.target.value)}>
            {ROLES.map(r => <option key={r}>{r}</option>)}
          </select>
        </div>
      </header>

      <div className="layout">
        <aside className="sidebar">
          <CreateLoan onCreated={(id) => { setSelected(id); refresh(); }} />
          <h3>Loans ({loans.length})</h3>
          <ul className="loan-list">
            {loans.map((l: any) => (
              <li key={l.id}
                  className={`loan-item ${selected === l.id ? 'active' : ''}`}
                  onClick={() => setSelected(l.id)}>
                <span className={`dot state-${l.state.toLowerCase()}`} />
                <span className="loan-id">{l.id.slice(0, 8)}</span>
                <span className="loan-state">{l.state}</span>
              </li>
            ))}
          </ul>
        </aside>

        <main className="main">
          {selected
            ? <ProjectionView resourceId={selected} role={role} onAction={refresh} />
            : <div className="empty">Select or create a loan</div>
          }
        </main>
      </div>
    </div>
  );
}

// ── Create Loan ────────────────────────────────────────────────────────

function CreateLoan({ onCreated }: { onCreated: (id: string) => void }) {
  const [amount, setAmount] = useState('500000');

  const create = async () => {
    const res = await fetch(`${API}/api/loan`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ data: { amount: Number(amount), purpose: 'working_capital' }, actor: 'ui', authority_level: 'SYSTEM' }),
    });
    const data = await res.json();
    if (data.id) onCreated(data.id);
  };

  return (
    <div className="create-box">
      <input type="number" value={amount} onChange={e => setAmount(e.target.value)} placeholder="Amount" />
      <button onClick={create}>+ New Loan</button>
    </div>
  );
}

// ── Projection View ────────────────────────────────────────────────────

function ProjectionView({ resourceId, role, onAction }: {
  resourceId: string; role: string; onAction: () => void;
}) {
  const [proj, setProj] = useState<Projection | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(async () => {
    const res = await fetch(`${API}/api/interface/loan/${resourceId}?role=${role}`);
    if (res.ok) setProj(await res.json());
  }, [resourceId, role]);

  useEffect(() => { load(); }, [load]);

  // WebSocket
  useEffect(() => {
    let ws: WebSocket;
    try {
      ws = new WebSocket(`ws://localhost:8000/ws/interface/loan/${resourceId}`);
      ws.onopen = () => ws.send(JSON.stringify({ role }));
      ws.onmessage = (e) => {
        try { const d = JSON.parse(e.data); if (d.resource) setProj(d); } catch {}
      };
    } catch {}
    return () => { try { ws?.close(); } catch {} };
  }, [resourceId, role]);

  const act = async (action: string) => {
    setBusy(action);
    const res = await fetch(`${API}/api/interface/loan/${resourceId}/action`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ action, actor: `${role}-user`, role, authority_level: 'HUMAN' }),
    });
    const data = await res.json();
    if (res.ok && data.projection) { setProj(data.projection); onAction(); }
    else if (!res.ok) alert(data.detail?.reason || JSON.stringify(data.detail));
    setBusy(null);
  };

  if (!proj) return <div className="loading">Loading...</div>;
  const { resource, valid_actions, blocked_actions, warnings, proposals, audit_summary } = proj;

  return (
    <div className="projection">
      {/* Header */}
      <div className="proj-header">
        <span className={`state-badge state-${resource.state.toLowerCase()}`}>{resource.state}</span>
        {resource.is_terminal && <span className="badge terminal">TERMINAL</span>}
        <span className="badge version">v{resource.version}</span>
        <span className="badge id">{resource.id.slice(0, 8)}</span>
      </div>

      {/* Warnings */}
      {warnings.map((w, i) => (
        <div key={i} className={`alert alert-${w.severity}`}>
          <strong>{w.source}:</strong> {w.message}
        </div>
      ))}

      {/* Proposals */}
      {proposals.length > 0 && (
        <div className="card">
          <h3>AI Recommendations</h3>
          {proposals.map((p, i) => (
            <div key={i} className="proposal">
              <span className="agent">{p.agent}</span>
              <span className={`conf ${p.confidence > 0.8 ? 'high' : p.confidence > 0.5 ? 'med' : 'low'}`}>
                {(p.confidence * 100).toFixed(0)}%
              </span>
              <span className="reason">{p.reasoning}</span>
            </div>
          ))}
        </div>
      )}

      {/* Actions */}
      <div className="card">
        <h3>Actions</h3>
        {valid_actions.length === 0 ? (
          <p className="muted">
            {resource.is_terminal ? 'Terminal state' : `No actions for role "${role}"`}
          </p>
        ) : (
          <div className="actions">
            {valid_actions.map((a) => (
              <button key={a.action} className="btn-action" onClick={() => act(a.action)} disabled={!!busy}>
                {busy === a.action ? '...' : `→ ${a.action}`}
              </button>
            ))}
          </div>
        )}
      </div>

      {/* Blocked */}
      {blocked_actions.length > 0 && (
        <div className="card">
          <h3>Blocked</h3>
          {blocked_actions.map((b, i) => (
            <div key={i} className="blocked">
              <span className="blocked-action">{b.action}</span>
              <span className="blocked-by">{b.blocked_by}</span>
              <span className="blocked-reason">{b.reason}</span>
            </div>
          ))}
        </div>
      )}

      {/* Data */}
      <div className="card">
        <h3>Data {Object.keys(resource.data).length === 0 && <span className="muted">(restricted)</span>}</h3>
        <table>
          <tbody>
            {Object.entries(resource.data).map(([k, v]) => (
              <tr key={k}><td>{k}</td><td>{String(v)}</td></tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Audit */}
      {audit_summary.length > 0 && (
        <div className="card">
          <h3>Audit Trail</h3>
          <table>
            <thead><tr><th>From</th><th>To</th><th>Actor</th><th>Auth</th></tr></thead>
            <tbody>
              {audit_summary.map((a, i) => (
                <tr key={i}>
                  <td>{a.from_state}</td><td><strong>{a.to_state}</strong></td>
                  <td>{a.actor}</td><td><span className={`badge auth-${a.authority_level.toLowerCase()}`}>{a.authority_level}</span></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

export default App;
