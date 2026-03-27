import React, { useState, useEffect, useCallback } from 'react';
import './App.css';

const API = '';
const ROLES = ['data_entry', 'kyc_officer', 'document_officer', 'underwriter', 'senior_underwriter', 'branch_manager', 'collections', 'customer'];

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

// The lifecycle steps with the role that performs each
const LIFECYCLE_STEPS: [string, string, string][] = [
  ['APPLIED', 'data_entry', 'data_entry-1'],
  ['KYC_REVIEW', 'kyc_officer', 'kyc-1'],
  ['DOCUMENT_VERIFICATION', 'kyc_officer', 'kyc-1'],
  ['CREDIT_BUREAU_CHECK', 'document_officer', 'doc-1'],
  ['UNDERWRITING', 'document_officer', 'doc-1'],
  // Agent auto-approves here if amount < 1M
  ['DISBURSED', 'branch_manager', 'bm-1'],
  ['REPAYING', 'branch_manager', 'bm-1'],
  ['CLOSED', 'branch_manager', 'bm-1'],
];

function App() {
  const [role, setRole] = useState('data_entry');
  const [loans, setLoans] = useState<any[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  const [running, setRunning] = useState(false);
  const [log, setLog] = useState<string[]>([]);

  const refresh = () => setTick(t => t + 1);
  const addLog = (msg: string) => setLog(prev => [...prev, msg]);

  useEffect(() => {
    fetch(`${API}/api/loan`).then(r => r.json()).then(setLoans).catch(() => {});
  }, [tick]);

  const runLifecycle = async () => {
    setRunning(true);
    setLog([]);
    addLog('Creating applicant...');

    // Create applicant
    const appRes = await fetch(`${API}/api/applicant`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ data: { name: 'Demo Corp Pvt Ltd' }, actor: 'system', authority_level: 'SYSTEM' }),
    });
    const app = await appRes.json();
    addLog(`✓ Applicant: ${app.id?.slice(0, 8)}`);

    // Create loan
    const loanRes = await fetch(`${API}/api/loan`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        data: { amount: 800000, purpose: 'working_capital', tenure: 24, interest_rate: 12.5, applicant_id: app.id },
        actor: 'system', authority_level: 'SYSTEM',
      }),
    });
    const loan = await loanRes.json();
    const lid = loan.id;
    setSelected(lid);
    setRole('data_entry');
    refresh();
    addLog(`✓ Loan created: ₹8,00,000 (${lid?.slice(0, 8)})`);
    await delay(800);

    // Walk through lifecycle
    for (const [targetState, stepRole, actor] of LIFECYCLE_STEPS) {
      setRole(stepRole);
      await delay(400);

      // Check if already past this state (agent may have auto-approved)
      const checkRes = await fetch(`${API}/api/loan/${lid}`);
      const current = await checkRes.json();

      // If the resource is already at or past the target, skip
      if (current.state === targetState || current.state === 'APPROVED' || current.state === 'CLOSED') {
        if (current.state === 'APPROVED' && targetState !== 'DISBURSED' && targetState !== 'REPAYING' && targetState !== 'CLOSED') {
          addLog(`⚡ Agent auto-approved! Skipping ${targetState}`);
          continue;
        }
        if (current.state === 'CLOSED') {
          addLog(`✓ Already CLOSED`);
          break;
        }
      }

      addLog(`→ ${stepRole}: transitioning to ${targetState}...`);

      const res = await fetch(`${API}/api/interface/loan/${lid}/action`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ action: targetState, actor, role: stepRole, authority_level: 'HUMAN' }),
      });
      const result = await res.json();

      if (res.ok && result.projection) {
        const newState = result.projection.resource.state;
        addLog(`✓ ${newState} (v${result.projection.resource.version})`);

        // Check if agent auto-approved during this transition
        if (newState === 'APPROVED' && targetState === 'UNDERWRITING') {
          addLog(`⚡ Decision node auto-approved!`);
          // Show agent proposals if any
          if (result.projection.proposals?.length > 0) {
            for (const p of result.projection.proposals) {
              addLog(`  🤖 ${p.agent}: ${(p.confidence * 100).toFixed(0)}% — ${p.reasoning}`);
            }
          }
        }
      } else {
        addLog(`✗ Blocked: ${result.detail?.reason || JSON.stringify(result.detail)}`);
        // Try to continue anyway
      }

      refresh();
      await delay(1000);
    }

    // Final state
    const finalRes = await fetch(`${API}/api/loan/${lid}`);
    const final = await finalRes.json();
    addLog(`\n✅ Final state: ${final.state} (version ${final.version})`);

    // Show audit trail
    const auditRes = await fetch(`${API}/api/loan/${lid}/audit`);
    const audit = await auditRes.json();
    addLog(`📋 Audit trail: ${audit.length} records`);
    for (const a of audit) {
      addLog(`  ${a.previous_state} → ${a.new_state} by ${a.actor} (${a.authority_level})`);
    }

    setRunning(false);
    refresh();
  };

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
        <button className="btn-lifecycle" onClick={runLifecycle} disabled={running}>
          {running ? '⏳ Running...' : '▶ Run Full Lifecycle'}
        </button>
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

          {/* Activity log */}
          {log.length > 0 && (
            <div className="log-section">
              <h3>Activity</h3>
              <div className="log">
                {log.map((msg, i) => <div key={i} className="log-entry">{msg}</div>)}
              </div>
            </div>
          )}
        </aside>

        <main className="main">
          {selected
            ? <ProjectionView resourceId={selected} role={role} onAction={refresh} />
            : <div className="empty">Select a loan or click "Run Full Lifecycle"</div>
          }
        </main>
      </div>
    </div>
  );
}

function delay(ms: number) { return new Promise(r => setTimeout(r, ms)); }

function CreateLoan({ onCreated }: { onCreated: (id: string) => void }) {
  const [amount, setAmount] = useState('500000');
  const create = async () => {
    const res = await fetch(`${API}/api/loan`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ data: { amount: Number(amount), purpose: 'working_capital' }, actor: 'ui', authority_level: 'SYSTEM' }),
    });
    const data = await res.json();
    if (data.id) onCreated(data.id);
  };
  return (
    <div className="create-box">
      <input type="number" value={amount} onChange={e => setAmount(e.target.value)} />
      <button onClick={create}>+ New Loan</button>
    </div>
  );
}

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

  // WebSocket for real-time
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
      method: 'POST', headers: { 'Content-Type': 'application/json' },
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
      <div className="proj-header">
        <span className={`state-badge state-${resource.state.toLowerCase()}`}>{resource.state}</span>
        {resource.is_terminal && <span className="badge terminal">TERMINAL</span>}
        <span className="badge version">v{resource.version}</span>
        <span className="badge id">{resource.id.slice(0, 8)}</span>
      </div>

      {warnings.map((w, i) => (
        <div key={i} className={`alert alert-${w.severity}`}>
          <strong>{w.source}:</strong> {w.message}
        </div>
      ))}

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

      <div className="card">
        <h3>Actions</h3>
        {valid_actions.length === 0 ? (
          <p className="muted">{resource.is_terminal ? 'Terminal state' : `No actions for "${role}"`}</p>
        ) : (
          <div className="actions">
            {valid_actions.map((a) => (
              <button key={a.action} className="btn-action" onClick={() => act(a.action)} disabled={!!busy}>
                {busy === a.action ? '⏳' : `→ ${a.action}`}
              </button>
            ))}
          </div>
        )}
      </div>

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

      <div className="card">
        <h3>Data {Object.keys(resource.data).length === 0 && <span className="muted">(restricted for this role)</span>}</h3>
        <table>
          <tbody>
            {Object.entries(resource.data).map(([k, v]) => (
              <tr key={k}><td>{k}</td><td>{typeof v === 'object' ? JSON.stringify(v) : String(v)}</td></tr>
            ))}
          </tbody>
        </table>
      </div>

      {audit_summary.length > 0 && (
        <div className="card">
          <h3>Audit Trail ({audit_summary.length})</h3>
          <table>
            <thead><tr><th>From</th><th>To</th><th>Actor</th><th>Authority</th></tr></thead>
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
