/* ============================================
   Eidolon Dashboard — Application Logic
   ============================================ */

const API_BASE = window.location.port === '5173'
  ? 'http://localhost:8545'
  : (import.meta.env.VITE_API_URL || 'https://eidolon-production-fae5.up.railway.app');

// ---- API Client ----

async function api(path, opts = {}) {
  const url = `${API_BASE}${path}`;
  const res = await fetch(url, {
    headers: { 'Content-Type': 'application/json', ...opts.headers },
    ...opts,
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(err.error || `HTTP ${res.status}`);
  }
  return res.json();
}

const eidolon = {
  health: () => api('/health'),
  // Forks
  createFork: (data) => api('/api/forks', { method: 'POST', body: JSON.stringify(data) }),
  listForks: () => api('/api/forks'),
  deleteFork: (id) => api(`/api/forks/${id}`, { method: 'DELETE' }),
  snapshotFork: (id) => api(`/api/forks/${id}/snapshot`, { method: 'POST' }),
  restoreFork: (id, snapId) => api(`/api/forks/${id}/restore/${snapId}`, { method: 'POST' }),
  // Keys
  createKey: (data) => api('/api/keys', { method: 'POST', body: JSON.stringify(data) }),
  listKeys: () => api('/api/keys'),
  deleteKey: (key) => api(`/api/keys/${key}`, { method: 'DELETE' }),
  // Usage
  usage: () => api('/api/usage'),
  // RPC
  rpc: (forkId, method, params = []) => api(`/rpc/${forkId}`, {
    method: 'POST',
    body: JSON.stringify({ jsonrpc: '2.0', method, params, id: 1 }),
  }),
};

// ---- Navigation ----

window.showPage = function(pageName) {
  document.querySelectorAll('.page').forEach(p => p.classList.remove('active'));
  document.querySelectorAll('.nav-link').forEach(l => l.classList.remove('active'));

  const page = document.getElementById(`page-${pageName}`);
  const link = document.getElementById(`nav-${pageName}`);
  if (page) page.classList.add('active');
  if (link) link.classList.add('active');

  // Load data for the page
  if (pageName === 'overview') refreshOverview();
  else if (pageName === 'forks') refreshForks();
  else if (pageName === 'keys') refreshKeys();
  else if (pageName === 'simulate') refreshSimForks();
};

document.querySelectorAll('.nav-link').forEach(link => {
  link.addEventListener('click', (e) => {
    e.preventDefault();
    showPage(link.dataset.page);
  });
});

// ---- Toast ----

window.showToast = function(message, type = 'success') {
  const toast = document.getElementById('toast');
  toast.textContent = message;
  toast.className = `toast ${type}`;
  setTimeout(() => toast.classList.add('hidden'), 3000);
};

// ---- Modals ----

window.showCreateForkModal = () => document.getElementById('modal-create-fork').classList.remove('hidden');
window.showCreateKeyModal = () => document.getElementById('modal-create-key').classList.remove('hidden');
window.closeModals = () => document.querySelectorAll('.modal').forEach(m => m.classList.add('hidden'));

// ---- Overview ----

window.refreshOverview = async function() {
  try {
    const [health, forksData, keysData] = await Promise.all([
      eidolon.health(),
      eidolon.listForks().catch(() => ({ forks: [], count: 0})),
      eidolon.listKeys().catch(() => ({ keys: [], count: 0})),
    ]);

    document.getElementById('stat-version').textContent = `v${health.version}`;
    document.getElementById('stat-forks').textContent = forksData.count;
    document.getElementById('stat-keys').textContent = keysData.count;

    try {
      const usage = await eidolon.usage();
      document.getElementById('stat-requests').textContent = formatNumber(usage.total_requests);
    } catch {
      document.getElementById('stat-requests').textContent = '0';
    }

    // Forks table
    const container = document.getElementById('overview-forks-list');
    if (forksData.forks.length === 0) {
      container.innerHTML = '<p class="empty-state">No forks yet. Create one to get started!</p>';
    } else {
      container.innerHTML = `
        <table>
          <thead><tr><th>Fork ID</th><th>Chain</th><th>Block</th><th>RPC</th></tr></thead>
          <tbody>${forksData.forks.map(f => `
            <tr>
              <td style="font-family:var(--font-mono);color:var(--accent-hover)">${f.id}</td>
              <td>${chainName(f.chain_id)}</td>
              <td style="font-family:var(--font-mono)">${f.block_number}</td>
              <td style="font-family:var(--font-mono);font-size:11px;color:var(--text-muted)">${f.rpc_endpoint}</td>
            </tr>
          `).join('')}</tbody>
        </table>
      `;
    }

    updateServerStatus(true);
  } catch (e) {
    updateServerStatus(false);
  }
};

// ---- Forks ----

window.refreshForks = async function() {
  try {
    const data = await eidolon.listForks();
    const container = document.getElementById('forks-list');

    if (data.forks.length === 0) {
      container.innerHTML = `
        <div class="card" style="grid-column:1/-1;text-align:center;padding:60px 20px">
          <p style="font-size:48px;margin-bottom:16px">🔱</p>
          <p style="font-size:18px;margin-bottom:8px;color:var(--text-primary)">No forks yet</p>
          <p style="margin-bottom:20px;color:var(--text-muted)">Create your first fork to start simulating</p>
          <button class="btn btn-primary" onclick="showCreateForkModal()">+ Create Fork</button>
        </div>
      `;
      return;
    }

    container.innerHTML = data.forks.map(fork => `
      <div class="fork-card">
        <div class="fork-card-header">
          <span class="fork-name">${fork.id}</span>
          <span class="fork-chain-badge">${chainName(fork.chain_id)}</span>
        </div>
        <div class="fork-meta">
          <div>
            <div class="fork-meta-item">Block Number</div>
            <div class="fork-meta-value">${fork.block_number}</div>
          </div>
          <div>
            <div class="fork-meta-item">Chain ID</div>
            <div class="fork-meta-value">${fork.chain_id}</div>
          </div>
        </div>
        <div class="fork-rpc" onclick="copyToClipboard('${fork.rpc_endpoint}')" title="Click to copy">
          📋 ${fork.rpc_endpoint}
        </div>
        <div class="fork-actions">
          <button class="btn btn-success btn-sm" onclick="snapshotFork('${fork.id}')">📸 Snapshot</button>
          <button class="btn btn-ghost btn-sm" onclick="promptRestore('${fork.id}')">⏪ Restore</button>
          <button class="btn btn-danger btn-sm" onclick="deleteFork('${fork.id}')">🗑 Delete</button>
        </div>
      </div>
    `).join('');
  } catch (e) {
    showToast('Failed to load forks: ' + e.message, 'error');
  }
};

window.createFork = async function() {
  try {
    const rpcUrl = document.getElementById('fork-rpc-url').value;
    const chainId = parseInt(document.getElementById('fork-chain-id').value) || 1;
    const blockNum = document.getElementById('fork-block').value;
    const customId = document.getElementById('fork-custom-id').value;

    if (!rpcUrl) { showToast('RPC URL is required', 'error'); return; }

    const body = {
      rpc_url: rpcUrl,
      chain_id: chainId,
    };
    if (blockNum) body.block_number = parseInt(blockNum);
    if (customId) body.fork_id = customId;

    await eidolon.createFork(body);
    closeModals();
    showToast('Fork created!', 'success');
    refreshForks();
    refreshOverview();
  } catch (e) {
    showToast('Failed: ' + e.message, 'error');
  }
};

window.deleteFork = async function(id) {
  if (!confirm(`Delete fork "${id}"?`)) return;
  try {
    await eidolon.deleteFork(id);
    showToast('Fork deleted', 'success');
    refreshForks();
  } catch (e) {
    showToast('Failed: ' + e.message, 'error');
  }
};

window.snapshotFork = async function(id) {
  try {
    const result = await eidolon.snapshotFork(id);
    showToast(`Snapshot created: ID ${result.snapshot_id}`, 'success');
  } catch (e) {
    showToast('Failed: ' + e.message, 'error');
  }
};

window.promptRestore = function(id) {
  const snapId = prompt('Enter snapshot ID to restore:');
  if (snapId === null) return;
  restoreFork(id, parseInt(snapId));
};

async function restoreFork(id, snapId) {
  try {
    await eidolon.restoreFork(id, snapId);
    showToast(`Restored fork to snapshot ${snapId}`, 'success');
  } catch (e) {
    showToast('Failed: ' + e.message, 'error');
  }
}

// ---- API Keys ----

window.refreshKeys = async function() {
  try {
    const data = await eidolon.listKeys();
    const container = document.getElementById('keys-list');

    if (data.keys.length === 0) {
      container.innerHTML = '<p class="empty-state">No API keys yet. Create one to authenticate requests.</p>';
      return;
    }

    container.innerHTML = `
      <table>
        <thead><tr><th>Key</th><th>Name</th><th>Requests</th><th>Rate Limit</th><th>Actions</th></tr></thead>
        <tbody>${data.keys.map(k => `
          <tr>
            <td style="font-family:var(--font-mono);font-size:11px">${k.key.slice(0, 12)}...${k.key.slice(-4)}</td>
            <td>${k.name}</td>
            <td style="font-family:var(--font-mono)">${formatNumber(k.request_count)}</td>
            <td>${k.rate_limit === 0 ? '<span style="color:var(--text-muted)">Unlimited</span>' : `${k.rate_limit}/min`}</td>
            <td><button class="btn btn-danger btn-sm" onclick="deleteKey('${k.key}')">Delete</button></td>
          </tr>
        `).join('')}</tbody>
      </table>
    `;
  } catch (e) {
    showToast('Failed to load keys: ' + e.message, 'error');
  }
};

window.createKey = async function() {
  try {
    const name = document.getElementById('key-name').value;
    const rateLimit = parseInt(document.getElementById('key-rate-limit').value) || 0;

    if (!name) { showToast('Key name is required', 'error'); return; }

    const result = await eidolon.createKey({ name, rate_limit: rateLimit });
    closeModals();
    showToast('Key created! Save it — it won\'t be shown again.', 'success');

    // Show the key in an alert since it's shown only once
    alert(`Your API Key:\n\n${result.key}\n\nSave this — it won't be shown again!`);
    refreshKeys();
  } catch (e) {
    showToast('Failed: ' + e.message, 'error');
  }
};

window.deleteKey = async function(key) {
  if (!confirm('Delete this API key?')) return;
  try {
    await eidolon.deleteKey(key);
    showToast('Key deleted', 'success');
    refreshKeys();
  } catch (e) {
    showToast('Failed: ' + e.message, 'error');
  }
};

// ---- Simulation ----

async function refreshSimForks() {
  try {
    const data = await eidolon.listForks();
    const select = document.getElementById('sim-fork');
    select.innerHTML = data.forks.map(f =>
      `<option value="${f.id}">${f.id} (${chainName(f.chain_id)})</option>`
    ).join('');
    if (data.forks.length === 0) {
      select.innerHTML = '<option disabled>No forks available — create one first</option>';
    }
  } catch (e) {
    console.error('Failed to load forks for sim', e);
  }
}

window.runSimulation = async function() {
  const forkId = document.getElementById('sim-fork').value;
  const from = document.getElementById('sim-from').value;
  const to = document.getElementById('sim-to').value;
  const value = document.getElementById('sim-value').value || '0x0';
  const data = document.getElementById('sim-data').value || '0x';

  if (!forkId) { showToast('Select a fork first', 'error'); return; }

  const resultsCard = document.getElementById('sim-results-card');
  const resultsDiv = document.getElementById('sim-results');
  resultsCard.style.display = 'block';
  resultsDiv.innerHTML = '<p style="color:var(--text-muted)">Simulating...</p>';

  try {
    const params = {};
    if (from) params.from = from;
    if (to) params.to = to;
    params.value = value;
    params.data = data;

    const response = await eidolon.rpc(forkId, 'eidolon_simulateTransaction', [params]);

    if (response.error) {
      resultsDiv.innerHTML = `<div class="result-badge failed">Error</div>
        <div class="result-value" style="margin-top:12px;color:var(--danger)">${response.error.message}</div>`;
      return;
    }

    const r = response.result;
    resultsDiv.innerHTML = `
      <div class="result-section">
        <div class="result-badge ${r.success ? 'success' : 'failed'}">${r.success ? '✓ Success' : '✗ Reverted'}</div>
      </div>
      <div class="result-section">
        <div class="result-label">Gas Used</div>
        <div class="result-value">${formatNumber(r.gas_used)}</div>
      </div>
      ${r.decoded_call ? `
        <div class="result-section">
          <div class="result-label">Function</div>
          <div class="result-value">${r.decoded_call.function_name}</div>
        </div>
      ` : ''}
      <div class="result-section">
        <div class="result-label">Return Data</div>
        <div class="result-value" style="font-size:11px;max-height:100px;overflow-y:auto">${r.return_data || '0x'}</div>
      </div>
      ${r.state_diffs && r.state_diffs.length > 0 ? `
        <div class="result-section">
          <div class="result-label">State Diffs (${r.state_diffs.length} accounts)</div>
          <div class="state-diff">${r.state_diffs.map(d => `
            <div style="margin-bottom:8px">
              <div style="color:var(--accent-hover)">${d.address}</div>
              <div>Balance: ${d.balance_before} → ${d.balance_after}</div>
              <div>Nonce: ${d.nonce_before} → ${d.nonce_after}</div>
              ${d.storage_diffs.map(s => `<div style="color:var(--text-muted)">  [${s.slot}]: ${s.before} → ${s.after}</div>`).join('')}
            </div>
          `).join('')}</div>
        </div>
      ` : ''}
      ${r.logs && r.logs.length > 0 ? `
        <div class="result-section">
          <div class="result-label">Logs (${r.logs.length})</div>
          <div class="state-diff">${JSON.stringify(r.logs, null, 2)}</div>
        </div>
      ` : ''}
    `;
  } catch (e) {
    resultsDiv.innerHTML = `<div class="result-badge failed">Error</div>
      <div class="result-value" style="margin-top:12px;color:var(--danger)">${e.message}</div>`;
  }
};

// ---- Utilities ----

function chainName(id) {
  const chains = { 1: 'Ethereum', 137: 'Polygon', 10: 'Optimism', 42161: 'Arbitrum', 8453: 'Base', 56: 'BSC', 43114: 'Avalanche' };
  return chains[id] || `Chain ${id}`;
}

function formatNumber(n) {
  if (n === undefined || n === null) return '0';
  return n.toLocaleString();
}

window.copyToClipboard = function(text) {
  navigator.clipboard.writeText(text);
  showToast('Copied to clipboard!', 'success');
};

function updateServerStatus(online) {
  const el = document.getElementById('server-status');
  const dot = el.querySelector('.status-dot');
  if (online) {
    dot.className = 'status-dot online';
    el.querySelector('span:last-child').textContent = 'Connected';
  } else {
    dot.className = 'status-dot offline';
    el.querySelector('span:last-child').textContent = 'Disconnected';
  }
}

// ---- Init ----
refreshOverview();

// Auto-refresh every 30 seconds
setInterval(() => {
  const activePage = document.querySelector('.page.active');
  if (activePage?.id === 'page-overview') refreshOverview();
}, 30000);
