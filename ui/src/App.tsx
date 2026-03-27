import React, { useState, useEffect, useCallback } from 'react';
import './App.css';

const API = '';

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

interface Spec {
  types: { name: string; states: string[]; transitions: { from: string; to: string }[] }[];
  policy_count: number; invariant_count: number; agent_count: number; decision_node_count: number;
}

function App() {
  const [spec, setSpec] = useState<Spec | null>(null);
  const [resourceType, setResourceType] = useState('');
  const [role, setRole] = useState('');
  const [roles, setRoles] = useState<string[]>([]);
  const [items, setItems] = useState<any[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  const [running, setRunning] = useState(false);
  const [log, setLog] = useState<string[]>([]);

  const refresh = () => setTick(t => t + 1);
  const addLog = (msg: string) => setLog(prev => [...prev, msg]);

  // Load spec on mount
  useEffect(() => {
    fetch(`${API}/api/spec`).then(r => r.json()).then((s: Spec) => {
      setSpec(s);
      if (s.types.length > 0) setResourceType(s.types[0].name);
    }).catch(() => {});
  }, []);

  // Discover roles from projection attempts
  useEffect(() => {
    if (!resourceType) return;
    // Try common role names to discover what's registered
    const tryRoles = [
      'applicant', 'intake_clerk', 'zoning_officer', 'env_officer',
      'structural_engineer', 'fire_marshal', 'building_inspector', 'planning_director',
      'data_entry', 'kyc_officer', 'document_officer', 'underwriter',
      'senior_underwriter', 'branch_manager', 'collections', 'customer',
      'maker', 'checker', 'approver', 'manager', 'auditor', 'viewer',
      'buyer', 'finance', 'admin',
    ];
    setRoles(tryRoles);
    if (!role) setRole(tryRoles[0]);
  }, [resourceType]);

  // Load items
  useEffect(() => {
    if (!resourceType) return;
    fetch(`${API}/api/${resourceType}`).then(r => r.json()).then(setItems).catch(() => setItems([]));
  }, [resourceType, tick]);

  const runLifecycle = async () => {
    if (!spec || !resourceType) return;
    setRunning(true);
    setLog([]);

    const typeSpec = spec.types.find(t => t.name === resourceType);
    if (!typeSpec) { setRunning(false); return; }

    addLog(`System: ${typeSpec.states.length} states, ${typeSpec.transitions.length} transitions, ${spec.policy_count} policies`);

    // Create supporting resources if needed
    const otherTypes = spec.types.filter(t => t.name !== resourceType);
    const supportIds: Record<string, string> = {};
    for (const t of otherTypes) {
      const res = await fetch(`${API}/api/${t.name}`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ data: { name: `Demo ${t.name}` }, actor: 'system', authority_level: 'SYSTEM' }),
      });
      const d = await res.json();
      if (d.id) { supportIds[t.name] = d.id; addLog(`✓ Created ${t.name}: ${d.id.slice(0, 8)}`); }
    }

    // Create main resource with data that satisfies policies
    const createData: Record<string, any> = {
      project_name: 'Demo Project', project_type: 'single_family',
      estimated_cost: 500000, zone: 'residential', stories: 2,
      building_height_ft: 28, lot_size_sqft: 8000, lot_coverage_pct: 40,
      building_sqft: 3200, foundation_type: 'standard', fire_exits: 2,
      smoke_detectors: true, total_parking: 4, ada_parking: 0,
      setback_front: 30, setback_rear: 25, setback_side_left: 15, setback_side_right: 15,
      fee_paid: true, inspection_fee_paid: true,
      neighbors_notified: true, liability_insurance: true,
      utility_clearance: true, erosion_control_plan: true,
      boundary_survey: true, soil_test_report: true,
      as_built_drawings: true, max_occupancy_posted: true,
      sprinkler_system: true, fire_lane: true, fire_rated_walls: false,
      ada_entrance: true, ada_restroom: true,
      // For loan system
      amount: 800000, purpose: 'working_capital', tenure: 24, interest_rate: 12.5,
    };
    // Add foreign keys
    for (const [tname, tid] of Object.entries(supportIds)) {
      createData[`${tname}_id`] = tid;
    }

    const createRes = await fetch(`${API}/api/${resourceType}`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ data: createData, actor: 'system', authority_level: 'SYSTEM' }),
    });
    const created = await createRes.json();
    if (!created.id) { addLog(`✗ Create failed: ${JSON.stringify(created)}`); setRunning(false); return; }
    const rid = created.id;
    setSelected(rid);
    addLog(`✓ Created ${resourceType}: ${rid.slice(0, 8)} (${created.state})`);
    refresh();
    await delay(500);

    // Walk through transitions greedily
    const visited = new Set<string>();
    let steps = 0;
    const maxSteps = 30;

    while (steps < maxSteps) {
      const checkRes = await fetch(`${API}/api/${resourceType}/${rid}`);
      const current = await checkRes.json();

      if (visited.has(current.state) && current.state !== typeSpec.states[0]) {
        addLog(`↩ Already visited ${current.state}, stopping`);
        break;
      }
      visited.add(current.state);

      // Try each role to find one that has valid actions
      let acted = false;
      for (const tryRole of roles) {
        let projRes;
        try {
          projRes = await fetch(`${API}/api/interface/${resourceType}/${rid}?role=${tryRole}`);
          if (!projRes.ok) continue;
        } catch { continue; }
        const proj = await projRes.json();
        if (!proj.valid_actions || proj.valid_actions.length === 0) continue;

        // Pick first valid action that moves forward (not back to DRAFT/start)
        const action = proj.valid_actions.find((a: any) =>
          !visited.has(a.action) || a.action === typeSpec.states[typeSpec.states.length - 1]
        ) || proj.valid_actions[0];

        setRole(tryRole);
        addLog(`→ ${tryRole}: ${current.state} → ${action.action}`);

        const actRes = await fetch(`${API}/api/interface/${resourceType}/${rid}/action`, {
          method: 'POST', headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ action: action.action, actor: `${tryRole}-1`, role: tryRole, authority_level: 'HUMAN' }),
        });
        const actData = await actRes.json();

        if (actRes.ok && actData.projection) {
          const newState = actData.projection.resource.state;
          addLog(`  ✓ ${newState} (v${actData.projection.resource.version})`);

          if (actData.projection.proposals?.length > 0) {
            for (const p of actData.projection.proposals) {
              addLog(`  🤖 ${p.agent}: ${(p.confidence * 100).toFixed(0)}% — ${p.reasoning}`);
            }
          }

          if (actData.projection.resource.is_terminal) {
            addLog(`\n🏁 Terminal state: ${newState}`);
            acted = true;
            break;
          }
        } else {
          addLog(`  ✗ ${actData.detail?.reason || JSON.stringify(actData.detail)}`);
          continue;
        }

        refresh();
        await delay(700);
        acted = true;
        break;
      }

      if (!acted) {
        addLog(`⏸ No role can act from ${current.state}`);
        break;
      }

      steps++;

      // Check if we reached terminal
      const finalCheck = await fetch(`${API}/api/${resourceType}/${rid}`);
      const finalState = await finalCheck.json();
      if (typeSpec.transitions.filter(t => t.from === finalState.state).length === 0) {
        addLog(`\n🏁 No outbound transitions from ${finalState.state}`);
        break;
      }
    }

    // Final summary
    const auditRes = await fetch(`${API}/api/${resourceType}/${rid}/audit`);
    const audit = await auditRes.json();
    addLog(`\n📋 ${audit.length} audit records:`);
    for (const a of audit.slice(-10)) {
      addLog(`  ${a.previous_state} → ${a.new_state} by ${a.actor} (${a.authority_level})`);
    }

    setRunning(false);
    refresh();
  };

  if (!spec) return <div className="loading">Loading system spec...</div>;

  return (
    <div className="app">
      <header className="header">
        <h1>Reconcile</h1>
        <span className="subtitle">
          {spec.types.map(t => t.name).join(' + ')} — {spec.policy_count} policies
        </span>
        <div className="role-picker">
          <select value={resourceType} onChange={e => { setResourceType(e.target.value); setSelected(null); }}>
            {spec.types.map(t => <option key={t.name}>{t.name}</option>)}
          </select>
          <select value={role} onChange={e => setRole(e.target.value)}>
            {roles.map(r => <option key={r}>{r}</option>)}
          </select>
        </div>
        <button className="btn-lifecycle" onClick={runLifecycle} disabled={running}>
          {running ? '⏳ Running...' : '▶ Run Lifecycle'}
        </button>
      </header>

      <div className="layout">
        <aside className="sidebar">
          <h3>{resourceType} ({items.length})</h3>
          <ul className="loan-list">
            {items.map((l: any) => (
              <li key={l.id}
                  className={`loan-item ${selected === l.id ? 'active' : ''}`}
                  onClick={() => setSelected(l.id)}>
                <span className={`dot dot-auto`} style={{background: stateColor(l.state)}} />
                <span className="loan-id">{l.id.slice(0, 8)}</span>
                <span className="loan-state">{l.state}</span>
              </li>
            ))}
          </ul>

          {log.length > 0 && (
            <div className="log-section">
              <h3>Activity</h3>
              <div className="log">
                {log.map((msg, i) => <div key={i} className="log-entry">{msg}</div>)}
                <div ref={el => el?.scrollIntoView()} />
              </div>
            </div>
          )}
        </aside>

        <main className="main">
          {selected
            ? <ProjectionView resourceId={selected} resourceType={resourceType} role={role} onAction={refresh} />
            : <div className="empty">
                <p>Click "Run Lifecycle" to see the full flow</p>
                <p className="muted">{spec.types.length} types, {spec.policy_count} policies, {spec.invariant_count} invariants, {spec.agent_count} agents</p>
              </div>
          }
        </main>
      </div>
    </div>
  );
}

function stateColor(state: string): string {
  const s = state.toLowerCase();
  if (s.includes('draft') || s.includes('pending')) return '#6b7280';
  if (s.includes('review') || s.includes('inspection')) return '#eab308';
  if (s.includes('approved') || s.includes('granted') || s.includes('closed')) return '#22c55e';
  if (s.includes('rejected') || s.includes('revoked') || s.includes('npa')) return '#ef4444';
  if (s.includes('construction') || s.includes('disbursed') || s.includes('repaying')) return '#6366f1';
  return '#3b82f6';
}

function delay(ms: number) { return new Promise(r => setTimeout(r, ms)); }

function ProjectionView({ resourceId, resourceType, role, onAction }: {
  resourceId: string; resourceType: string; role: string; onAction: () => void;
}) {
  const [proj, setProj] = useState<Projection | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(async () => {
    const res = await fetch(`${API}/api/interface/${resourceType}/${resourceId}?role=${role}`);
    if (res.ok) setProj(await res.json());
  }, [resourceId, resourceType, role]);

  useEffect(() => { load(); }, [load]);

  useEffect(() => {
    let ws: WebSocket;
    try {
      ws = new WebSocket(`ws://localhost:8000/ws/interface/${resourceType}/${resourceId}`);
      ws.onopen = () => ws.send(JSON.stringify({ role }));
      ws.onmessage = (e) => {
        try { const d = JSON.parse(e.data); if (d.resource) setProj(d); } catch {}
      };
    } catch {}
    return () => { try { ws?.close(); } catch {} };
  }, [resourceId, resourceType, role]);

  const act = async (action: string) => {
    setBusy(action);
    const res = await fetch(`${API}/api/interface/${resourceType}/${resourceId}/action`, {
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
        <span className="state-badge" style={{background: stateColor(resource.state) + '33', color: stateColor(resource.state), border: `1px solid ${stateColor(resource.state)}`}}>
          {resource.state}
        </span>
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
        <h3>Actions ({valid_actions.length} valid, {blocked_actions.length} blocked)</h3>
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
          <h3>Blocked ({blocked_actions.length})</h3>
          <div style={{maxHeight: 200, overflow: 'auto'}}>
          {blocked_actions.map((b, i) => (
            <div key={i} className="blocked">
              <span className="blocked-action">{b.action}</span>
              <span className="blocked-by">{b.blocked_by}</span>
              <span className="blocked-reason">{b.reason}</span>
            </div>
          ))}
          </div>
        </div>
      )}

      <div className="card">
        <h3>Data ({Object.keys(resource.data).length} fields)</h3>
        <div style={{maxHeight: 250, overflow: 'auto'}}>
        <table>
          <tbody>
            {Object.entries(resource.data).map(([k, v]) => (
              <tr key={k}><td>{k}</td><td>{typeof v === 'object' ? JSON.stringify(v) : String(v)}</td></tr>
            ))}
          </tbody>
        </table>
        </div>
        {Object.keys(resource.data).length === 0 && <p className="muted">Restricted for this role</p>}
      </div>

      {audit_summary.length > 0 && (
        <div className="card">
          <h3>Audit Trail ({audit_summary.length})</h3>
          <div style={{maxHeight: 200, overflow: 'auto'}}>
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
        </div>
      )}
    </div>
  );
}

export default App;
