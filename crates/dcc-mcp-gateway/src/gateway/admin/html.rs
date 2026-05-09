//! Inline HTML dashboard for the admin UI.

/// The admin dashboard HTML page (inline CSS + vanilla JS, no external deps).
#[cfg(feature = "admin")]
pub const ADMIN_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>DCC-MCP Gateway Admin</title>
<style>
*{box-sizing:border-box;margin:0;padding:0}
body{font-family:monospace;background:#0d1117;color:#c9d1d9;display:flex;height:100vh;overflow:hidden}
nav{width:160px;background:#161b22;border-right:1px solid #30363d;display:flex;flex-direction:column;padding:12px 0;flex-shrink:0}
nav h1{font-size:12px;color:#58a6ff;padding:8px 16px 16px;border-bottom:1px solid #30363d;margin-bottom:8px;word-break:break-all}
nav a{display:block;padding:8px 16px;color:#8b949e;text-decoration:none;font-size:12px;border-left:3px solid transparent;transition:all .15s}
nav a:hover,nav a.active{color:#c9d1d9;background:#1c2129;border-left-color:#58a6ff}
main{flex:1;overflow-y:auto;padding:20px}
.panel{display:none}
.panel.active{display:block}
h2{font-size:14px;color:#58a6ff;margin-bottom:12px;padding-bottom:6px;border-bottom:1px solid #30363d}
.badge{display:inline-block;padding:2px 6px;border-radius:3px;font-size:11px;font-weight:bold}
.badge-ok{background:#1a3a1a;color:#3fb950}
.badge-err{background:#3a1a1a;color:#f85149}
.badge-warn{background:#3a2a00;color:#d29922}
table{width:100%;border-collapse:collapse;font-size:12px;margin-bottom:16px}
th{text-align:left;padding:6px 8px;background:#161b22;color:#8b949e;border-bottom:1px solid #30363d;white-space:nowrap}
td{padding:6px 8px;border-bottom:1px solid #21262d;vertical-align:top;word-break:break-all}
tr:hover td{background:#1c2129}
.status-bar{font-size:11px;color:#8b949e;margin-bottom:12px}
.refresh-btn{background:#21262d;border:1px solid #30363d;color:#c9d1d9;padding:4px 10px;font-size:11px;cursor:pointer;border-radius:3px;font-family:monospace}
.refresh-btn:hover{background:#30363d}
.empty{color:#484f58;font-size:12px;padding:12px 0}
.health-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(160px,1fr));gap:12px;margin-bottom:16px}
.health-card{background:#161b22;border:1px solid #30363d;border-radius:6px;padding:12px}
.health-card .label{font-size:11px;color:#8b949e;margin-bottom:4px}
.health-card .value{font-size:18px;font-weight:bold;color:#c9d1d9}
.health-card.ok{border-color:#238636}
.health-card.warn{border-color:#9e6a03}
.workers-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(280px,1fr));gap:12px;margin-bottom:16px}
.worker-card{background:#161b22;border:1px solid #30363d;border-radius:6px;padding:12px;font-size:11px;line-height:1.5}
.worker-card.ok{border-color:#238636}
.worker-card.stale{border-color:#9e6a03}
.worker-card.err{border-color:#f85149}
.worker-card .wname{font-size:13px;font-weight:bold;color:#c9d1d9;margin-bottom:6px;word-break:break-all}
.worker-card .wkv{display:grid;grid-template-columns:80px 1fr;gap:2px 8px;color:#8b949e}
.worker-card .wkv span:nth-child(2n){color:#c9d1d9;word-break:break-all}
.log-line{font-size:11px;padding:3px 0;border-bottom:1px solid #21262d;word-break:break-all}
.log-line:last-child{border-bottom:none}
pre{white-space:pre-wrap;word-break:break-all}
</style>
</head>
<body>
<nav>
  <h1>DCC-MCP<br>Gateway</h1>
  <a href="#" class="nav-link active" data-panel="health">Health</a>
  <a href="#" class="nav-link" data-panel="instances">Instances</a>
  <a href="#" class="nav-link" data-panel="tools">Tools</a>
  <a href="#" class="nav-link" data-panel="calls">Calls</a>
  <a href="#" class="nav-link" data-panel="workers">Workers</a>
  <a href="#" class="nav-link" data-panel="logs">Logs</a>
</nav>
<main>
  <!-- Health -->
  <div id="panel-health" class="panel active">
    <h2>Health</h2>
    <div id="health-status" class="status-bar">Loading…</div>
    <div id="health-grid" class="health-grid"></div>
    <button class="refresh-btn" onclick="fetchHealth()">Refresh</button>
  </div>
  <!-- Instances -->
  <div id="panel-instances" class="panel">
    <h2>Instances</h2>
    <div id="instances-status" class="status-bar">Loading…</div>
    <table><thead><tr>
      <th>ID</th><th>DCC</th><th>Status</th><th>Address</th><th>Scene</th>
    </tr></thead>
    <tbody id="instances-body"></tbody></table>
    <button class="refresh-btn" onclick="fetchInstances()">Refresh</button>
  </div>
  <!-- Tools -->
  <div id="panel-tools" class="panel">
    <h2>Tools</h2>
    <div id="tools-status" class="status-bar">Loading…</div>
    <table><thead><tr>
      <th>Slug</th><th>DCC</th><th>Summary</th>
    </tr></thead>
    <tbody id="tools-body"></tbody></table>
    <button class="refresh-btn" onclick="fetchTools()">Refresh</button>
  </div>
  <!-- Calls -->
  <div id="panel-calls" class="panel">
    <h2>Recent Calls</h2>
    <div id="calls-status" class="status-bar">Loading…</div>
    <table><thead><tr>
      <th>Time</th><th>Tool</th><th>DCC</th><th>Status</th><th>ms</th>
    </tr></thead>
    <tbody id="calls-body"></tbody></table>
    <button class="refresh-btn" onclick="fetchCalls()">Refresh</button>
  </div>
  <!-- Workers (Phase 4) -->
  <div id="panel-workers" class="panel">
    <h2>Workers</h2>
    <div id="workers-status" class="status-bar">Loading…</div>
    <div id="workers-grid" class="workers-grid"></div>
    <button class="refresh-btn" onclick="fetchWorkers()">Refresh</button>
  </div>
  <!-- Logs -->
  <div id="panel-logs" class="panel">
    <h2>Event Log</h2>
    <div id="logs-status" class="status-bar">Loading…</div>
    <div id="logs-body"></div>
    <button class="refresh-btn" onclick="fetchLogs()">Refresh</button>
  </div>
</main>
<script>
const BASE = location.origin + '/admin/api';
let activePanel = 'health';
const polls = {};

function show(panel) {
  document.querySelectorAll('.panel').forEach(p => p.classList.remove('active'));
  document.querySelectorAll('.nav-link').forEach(a => a.classList.remove('active'));
  document.getElementById('panel-' + panel).classList.add('active');
  document.querySelector('[data-panel="' + panel + '"]').classList.add('active');
  activePanel = panel;
  fetchPanel(panel);
}

document.querySelectorAll('.nav-link').forEach(a => {
  a.addEventListener('click', e => { e.preventDefault(); show(a.dataset.panel); });
});

function badge(val, ok_vals, warn_vals) {
  const v = String(val).toLowerCase();
  if (ok_vals && ok_vals.some(x => v.includes(x))) return '<span class="badge badge-ok">' + escHtml(val) + '</span>';
  if (warn_vals && warn_vals.some(x => v.includes(x))) return '<span class="badge badge-warn">' + escHtml(val) + '</span>';
  return '<span class="badge badge-err">' + escHtml(val) + '</span>';
}

function escHtml(s) {
  return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
}

function fmtTime(ts) {
  if (!ts) return '-';
  try { return new Date(ts).toLocaleTimeString(); } catch { return escHtml(ts); }
}

async function fetchHealth() {
  try {
    const r = await fetch(BASE + '/health');
    const d = await r.json();
    document.getElementById('health-status').textContent =
      'Last updated: ' + new Date().toLocaleTimeString();
    const ok = d.status === 'ok';
    const grid = document.getElementById('health-grid');
    grid.innerHTML = [
      card(ok ? 'ok' : 'warn', 'Status', escHtml(d.status || '?')),
      card('', 'Uptime', formatUptime(d.uptime_secs)),
      card(d.instances_ready > 0 ? 'ok' : 'warn', 'Ready', d.instances_ready + ' / ' + d.instances_total),
      card('', 'Version', escHtml(d.version || '?')),
    ].join('');
  } catch(e) {
    document.getElementById('health-status').textContent = 'Error: ' + e.message;
  }
}

function card(cls, label, value) {
  return '<div class="health-card ' + cls + '"><div class="label">' + label + '</div><div class="value">' + value + '</div></div>';
}

function formatUptime(secs) {
  if (secs == null) return '?';
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  return h + 'h ' + m + 'm ' + s + 's';
}

async function fetchInstances() {
  try {
    const r = await fetch(BASE + '/instances');
    const d = await r.json();
    const rows = d.instances || [];
    document.getElementById('instances-status').textContent =
      rows.length + ' instance(s) — ' + new Date().toLocaleTimeString();
    const tbody = document.getElementById('instances-body');
    if (!rows.length) { tbody.innerHTML = '<tr><td colspan="5" class="empty">No instances registered.</td></tr>'; return; }
    tbody.innerHTML = rows.map(e => '<tr>' +
      '<td>' + escHtml(String(e.id || e.instance_id || '').slice(0,8)) + '</td>' +
      '<td>' + escHtml(e.dcc_type || '?') + '</td>' +
      '<td>' + badge(e.status, ['ready','available'], ['stale','booting']) + '</td>' +
      '<td>' + escHtml((e.host || '') + ':' + (e.port || '')) + '</td>' +
      '<td>' + escHtml(e.scene || '-') + '</td>' +
    '</tr>').join('');
  } catch(e) {
    document.getElementById('instances-status').textContent = 'Error: ' + e.message;
  }
}

async function fetchTools() {
  try {
    const r = await fetch(BASE + '/tools');
    const d = await r.json();
    const rows = d.tools || [];
    document.getElementById('tools-status').textContent =
      rows.length + ' tool(s) — ' + new Date().toLocaleTimeString();
    const tbody = document.getElementById('tools-body');
    if (!rows.length) { tbody.innerHTML = '<tr><td colspan="3" class="empty">No tools registered.</td></tr>'; return; }
    tbody.innerHTML = rows.map(t => '<tr>' +
      '<td>' + escHtml(t.slug || t.name || '?') + '</td>' +
      '<td>' + escHtml(t.dcc_type || t.dcc || '-') + '</td>' +
      '<td>' + escHtml((t.summary || t.description || '').slice(0,120)) + '</td>' +
    '</tr>').join('');
  } catch(e) {
    document.getElementById('tools-status').textContent = 'Error: ' + e.message;
  }
}

async function fetchCalls() {
  try {
    const r = await fetch(BASE + '/calls');
    const d = await r.json();
    const rows = d.calls || [];
    document.getElementById('calls-status').textContent =
      rows.length + ' call(s) — ' + new Date().toLocaleTimeString();
    const tbody = document.getElementById('calls-body');
    if (!rows.length) { tbody.innerHTML = '<tr><td colspan="5" class="empty">No recent calls. AuditMiddleware may not be active.</td></tr>'; return; }
    tbody.innerHTML = rows.map(c => '<tr>' +
      '<td>' + fmtTime(c.timestamp) + '</td>' +
      '<td>' + escHtml(c.tool || c.action || '?') + '</td>' +
      '<td>' + escHtml(c.dcc_type || '-') + '</td>' +
      '<td>' + badge(c.status || (c.success ? 'ok' : 'err'), ['ok','success'], []) + '</td>' +
      '<td>' + escHtml(c.duration_ms != null ? c.duration_ms : '-') + '</td>' +
    '</tr>').join('');
  } catch(e) {
    document.getElementById('calls-status').textContent = 'Error: ' + e.message;
  }
}

async function fetchLogs() {
  try {
    const r = await fetch(BASE + '/logs');
    const d = await r.json();
    const rows = d.logs || d.events || [];
    document.getElementById('logs-status').textContent =
      rows.length + ' event(s) — ' + new Date().toLocaleTimeString();
    const body = document.getElementById('logs-body');
    if (!rows.length) { body.innerHTML = '<p class="empty">No events recorded.</p>'; return; }
    body.innerHTML = rows.map(l => '<div class="log-line">' +
      '<span style="color:#8b949e">' + fmtTime(l.timestamp || l.time) + '</span> ' +
      '<span style="color:#d29922">[' + escHtml(l.level || 'info') + ']</span> ' +
      escHtml(l.message || l.msg || JSON.stringify(l)) +
    '</div>').join('');
  } catch(e) {
    document.getElementById('logs-status').textContent = 'Error: ' + e.message;
  }
}

function workerCardClass(w) {
  if (w.stale) return 'stale';
  const s = String(w.status || '').toLowerCase();
  if (s.includes('available') || s.includes('busy') || s.includes('ready')) return 'ok';
  return 'err';
}

function formatBytes(n) {
  if (n == null) return '-';
  const units = ['B','KB','MB','GB','TB'];
  let i = 0; let v = Number(n);
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
  return v.toFixed(1) + ' ' + units[i];
}

async function fetchWorkers() {
  try {
    const r = await fetch(BASE + '/workers');
    const d = await r.json();
    const rows = d.workers || [];
    const sum = d.summary || {};
    document.getElementById('workers-status').textContent =
      rows.length + ' worker(s)' +
      ' (live ' + (sum.live || 0) + ', stale ' + (sum.stale || 0) +
      ', unhealthy ' + (sum.unhealthy || 0) + ') — ' +
      new Date().toLocaleTimeString();
    const grid = document.getElementById('workers-grid');
    if (!rows.length) {
      grid.innerHTML = '<p class="empty">No workers registered.</p>';
      return;
    }
    grid.innerHTML = rows.map(w => {
      const cls = workerCardClass(w);
      const id = String(w.instance_id || '').slice(0,8);
      const name = escHtml(w.display_name || (w.dcc_type + ' @ ' + (w.host||'') + ':' + (w.port||'')));
      return '<div class="worker-card ' + cls + '">' +
        '<div class="wname">' + name + ' <span style="color:#8b949e;font-weight:normal">' + escHtml(id) + '</span></div>' +
        '<div class="wkv">' +
          '<span>DCC</span><span>' + escHtml(w.dcc_type || '?') + '</span>' +
          '<span>Status</span><span>' + badge(w.status, ['available','busy','ready'], ['stale','booting']) + '</span>' +
          '<span>PID</span><span>' + escHtml(w.pid != null ? w.pid : '-') + '</span>' +
          '<span>Uptime</span><span>' + formatUptime(w.uptime_secs) + '</span>' +
          '<span>Version</span><span>' + escHtml(w.version || '-') + '</span>' +
          '<span>Adapter</span><span>' + escHtml(w.adapter_version || '-') + '</span>' +
          '<span>CPU%</span><span>' + (w.cpu_percent != null ? Number(w.cpu_percent).toFixed(1) : '—') + '</span>' +
          '<span>Memory</span><span>' + formatBytes(w.memory_bytes) + '</span>' +
          '<span>MCP URL</span><span>' + escHtml(w.mcp_url || '-') + '</span>' +
        '</div>' +
      '</div>';
    }).join('');
  } catch(e) {
    document.getElementById('workers-status').textContent = 'Error: ' + e.message;
  }
}

function fetchPanel(panel) {
  if (panel === 'health') fetchHealth();
  else if (panel === 'instances') fetchInstances();
  else if (panel === 'tools') fetchTools();
  else if (panel === 'calls') fetchCalls();
  else if (panel === 'workers') fetchWorkers();
  else if (panel === 'logs') fetchLogs();
}

// Auto-poll every 5s for active panel
setInterval(() => fetchPanel(activePanel), 5000);
fetchPanel('health');
</script>
</body>
</html>"##;
