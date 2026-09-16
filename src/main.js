import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import './styles.css';

const app = document.querySelector('#app');

// window.prompt() is unsupported in WKWebView — custom modal instead
function showPrompt(message, expectedPhrase) {
  return new Promise((resolve) => {
    const overlay = document.createElement('div');
    overlay.className = 'modal-overlay';
    overlay.innerHTML = `
      <div class="modal-box">
        <p class="modal-msg">${escapeHtml(message)}</p>
        <input class="modal-input" type="text" placeholder="Type ${escapeHtml(expectedPhrase)} to confirm" autocomplete="off" />
        <div class="modal-actions">
          <button class="modal-cancel">Cancel</button>
          <button class="modal-confirm danger" disabled>Confirm</button>
        </div>
      </div>`;
    document.body.appendChild(overlay);
    const input = overlay.querySelector('.modal-input');
    const confirmBtn = overlay.querySelector('.modal-confirm');
    input.addEventListener('input', () => { confirmBtn.disabled = input.value.trim() !== expectedPhrase; });
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && input.value.trim() === expectedPhrase) done(true);
      if (e.key === 'Escape') done(false);
    });
    confirmBtn.addEventListener('click', () => done(true));
    overlay.querySelector('.modal-cancel').addEventListener('click', () => done(false));
    function done(ok) { overlay.remove(); resolve(ok ? input.value.trim() : null); }
    requestAnimationFrame(() => input.focus());
  });
}

function showTextPrompt(message, initialValue = '') {
  return new Promise((resolve) => {
    const overlay = document.createElement('div');
    overlay.className = 'modal-overlay';
    overlay.innerHTML = `
      <div class="modal-box">
        <p class="modal-msg">${escapeHtml(message)}</p>
        <input class="modal-input" type="text" value="${escapeHtml(initialValue)}" autocomplete="off" />
        <div class="modal-actions">
          <button class="modal-cancel">Cancel</button>
          <button class="modal-confirm primary">Save</button>
        </div>
      </div>`;
    document.body.appendChild(overlay);
    const input = overlay.querySelector('.modal-input');
    const saveBtn = overlay.querySelector('.modal-confirm');
    const update = () => { saveBtn.disabled = !input.value.trim(); };
    input.addEventListener('input', update);
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && input.value.trim()) done(input.value.trim());
      if (e.key === 'Escape') done(null);
    });
    saveBtn.addEventListener('click', () => done(input.value.trim()));
    overlay.querySelector('.modal-cancel').addEventListener('click', () => done(null));
    function done(value) { overlay.remove(); resolve(value); }
    update();
    requestAnimationFrame(() => { input.focus(); input.select(); });
  });
}

// ── App state ─────────────────────────────────────────────────────────────────

const SAVED_SEARCHES_KEY = 'macos-dev-cache-cleaner.saved-project-searches';

function readSavedSearches() {
  try {
    const parsed = JSON.parse(localStorage.getItem(SAVED_SEARCHES_KEY) || '[]');
    return Array.isArray(parsed) ? parsed.filter((search) => search && search.id && search.name && search.root) : [];
  } catch (_) {
    return [];
  }
}

let state = {
  report: null, selected: new Set(),
  filter: '', riskFilter: 'all', categoryFilter: 'all',
  lastPlan: null, busy: false,
  activeRowKey: null,
  tree: { loading: false, nodes: [], breadcrumb: [], rootLabel: '' },
};

let scanState = {
  report: null,
  selected: new Set(),
  typeFilter: 'all',
  minSize: 10 * 1024 * 1024,   // 10 MB default
  staleDays: 0,                  // 0 = no age filter
  sortBy: 'size',
  sortDirection: 'desc',
  busy: false,
  acceptLiveBatches: false,
  liveRenderTimer: null,
  savedSearches: readSavedSearches(),
  activeSavedSearchId: null,
};

// ── Layout ────────────────────────────────────────────────────────────────────

app.innerHTML = `
  <div class="tab-bar">
    <button class="tab-btn active" data-tab="cache">System Cache Cleaner</button>
    <button class="tab-btn" data-tab="projects">Project Artifact Scanner</button>
    <button class="tab-btn" data-tab="info">How searches work</button>
    <div class="memory-badge" title="Resident memory used by the main Tauri process">
      <span>Memory</span><strong id="memoryUsage">—</strong>
    </div>
  </div>

  <!-- ═══ Tab 1: Cache Cleaner ═══ -->
  <div id="pageCacheCleaner" class="tab-page active">
    <main class="shell">
      <section class="hero card">
        <div>
          <p class="eyebrow">local-only macOS utility</p>
          <h1>Dev Cache Cleaner</h1>
          <p class="subline">Discover, dry-run, and clean developer caches for Homebrew, JetBrains, Ollama, Hugging Face, Docker, Python, Rust, Node, Android, and related AI tooling.</p>
        </div>
        <div class="hero-actions">
          <button id="discoverBtn" class="primary">Discover</button>
          <button id="dryRunBtn">Dry run selected</button>
          <button id="cleanBtn" class="danger">Clean selected</button>
        </div>
      </section>

      <section class="toolbar card">
        <label>Search
          <input id="filterInput" type="search" placeholder="ollama, docker, jetbrains…" />
        </label>
        <label>Risk
          <select id="riskFilter">
            <option value="all">All</option>
            <option value="low">Low</option>
            <option value="medium">Medium</option>
            <option value="high">High</option>
          </select>
        </label>
        <label>Category
          <select id="categoryFilter"><option value="all">All</option></select>
        </label>
        <div class="checks">
          <label><input id="moveToTrash" type="checkbox" checked /> Move to Trash</label>
          <label><input id="allowCommands" type="checkbox" /> Allow commands</label>
          <label><input id="includeHighRisk" type="checkbox" /> Include high-risk</label>
        </div>
      </section>

      <section class="stats">
        <div class="stat card"><span>Discovered</span><strong id="totalSize">—</strong></div>
        <div class="stat card"><span>Selected</span><strong id="selectedSize">—</strong></div>
        <div class="stat card"><span>Items</span><strong id="itemCount">—</strong></div>
        <div class="stat card"><span>Last scan</span><strong id="lastScan">—</strong></div>
      </section>

      <section class="quick card">
        <button id="selectDefaults">Select safe defaults</button>
        <button id="selectLow">Low risk only</button>
        <button id="selectNone">Select none</button>
        <button id="copyPlan" class="small">Copy last plan</button>
      </section>

      <section class="log-card card">
        <div class="log-header"><h2>Plan / log</h2></div>
        <div id="warnings" class="warnings"></div>
        <pre id="logBox">No actions yet.</pre>
      </section>

      <div class="main-grid">
        <section class="card table-card">
          <div class="table-header">
            <h2>Discovery results</h2>
            <span id="visibleCount">No scan yet</span>
          </div>
          <div class="table-wrap">
            <table>
              <thead>
                <tr>
                  <th class="col-sel"></th>
                  <th>Target</th>
                  <th class="col-cat">Category</th>
                  <th class="col-risk">Risk</th>
                  <th class="col-size">Size</th>
                  <th class="col-path">Path / command</th>
                </tr>
              </thead>
              <tbody id="itemsBody">
                <tr><td colspan="6" class="empty">Click Discover to scan known developer cache locations.</td></tr>
              </tbody>
            </table>
          </div>
        </section>

        <section class="card tree-card" id="treeCard">
          <div class="tree-header">
            <span class="tree-title" id="treeTitle">Contents</span>
            <button class="icon-btn" id="treeCloseBtn" title="Close">✕</button>
          </div>
          <nav class="tree-crumb" id="treeCrumb"></nav>
          <div class="tree-body" id="treeBody">
            <div class="tree-empty">Click any row to browse<br>its filesystem contents.</div>
          </div>
        </section>
      </div>
    </main>
  </div>

  <!-- ═══ Tab 2: Project Artifact Scanner ═══ -->
  <div id="pageProjectScanner" class="tab-page">
    <main class="shell">
      <section class="hero card">
        <div>
          <p class="eyebrow">project workspace scanner</p>
          <h1>Project Artifacts</h1>
          <p class="subline">Recursively find <code style="font-size:13px">node_modules</code>, <code style="font-size:13px">.venv</code>, Rust/Java build dirs, and similar artifact folders within a chosen root. Shows project context, sizes, and last-modified age so you can target stale builds.</p>
        </div>
        <div class="hero-actions">
          <button id="scanBtn" class="primary">Scan</button>
          <button id="deleteArtifactsBtn" class="danger">Delete selected</button>
        </div>
      </section>

      <section class="card scanner-config">
        <div class="scanner-config-grid">
          <label class="cfg-label">Root directory to scan
            <div class="dir-row">
              <input id="scanRoot" type="text" value="~/Code" placeholder="~/Code or ~/Projects" />
              <button id="chooseScanRoot" type="button">Choose folder…</button>
            </div>
          </label>
          <label class="cfg-label">Artifact type
            <select id="artifactTypeFilter"><option value="all">All types</option></select>
          </label>
          <label class="cfg-label">Min size
            <select id="artifactMinSize">
              <option value="0">Any size</option>
              <option value="1048576">≥ 1 MB</option>
              <option value="10485760" selected>≥ 10 MB</option>
              <option value="104857600">≥ 100 MB</option>
              <option value="1073741824">≥ 1 GB</option>
            </select>
          </label>
          <label class="cfg-label">Stale — not modified in
            <select id="artifactStaleFilter">
              <option value="0">Any age</option>
              <option value="30">1 month+</option>
              <option value="90">3 months+</option>
              <option value="180">6 months+</option>
              <option value="365" selected>1 year+</option>
            </select>
          </label>
          <label class="cfg-label">Sort by
            <select id="artifactSortBy">
              <option value="size">Size (largest first)</option>
              <option value="age">Age (oldest first)</option>
              <option value="modified">Last modified (newest first)</option>
              <option value="project">Project name</option>
              <option value="kind">Artifact type</option>
            </select>
          </label>
          <label class="cfg-label">Delete mode
            <select id="scanDeleteMode">
              <option value="trash">Move to Trash</option>
              <option value="permanent">Permanently delete</option>
            </select>
          </label>
        </div>
        <div class="saved-searches">
          <label class="cfg-label">Saved searches
            <div class="saved-search-row">
              <select id="savedSearchSelect"><option value="">Choose a saved search…</option></select>
              <button id="saveSearchBtn" type="button">Save current</button>
              <button id="deleteSavedSearchBtn" type="button" disabled>Delete</button>
            </div>
          </label>
          <span id="savedSearchStatus" class="info-note">Saved searches stay on this Mac.</span>
        </div>
      </section>

      <section class="stats scanner-stats">
        <div class="stat card"><span>Total reclaimable</span><strong id="scanTotalSize">—</strong></div>
        <div class="stat card"><span>Artifacts found</span><strong id="scanArtifactCount">—</strong></div>
        <div class="stat card"><span>Projects</span><strong id="scanProjectCount">—</strong></div>
        <div class="stat card"><span>Visible / selected</span><strong id="scanVisibleCount">—</strong></div>
        <div class="stat card"><span>Selected space</span><strong id="scanSelectedSize">—</strong></div>
      </section>

      <section class="quick card">
        <button id="selectStale">Select stale (≥ 1 yr)</button>
        <button id="selectHuge">Select large (≥ 500 MB)</button>
        <button id="selectAllVisible">Select all visible</button>
        <button id="selectNoneArtifacts">Select none</button>
      </section>

      <section class="log-card card">
        <div class="log-header"><h2>Scanner log</h2></div>
        <div id="scanProgress" class="scan-progress" hidden>
          <div class="scan-progress-label"><span id="scanProgressText"></span><span id="scanProgressCount"></span></div>
          <div class="scan-progress-track"><div id="scanProgressBar"></div></div>
          <div id="scanProgressPath" class="scan-progress-path"></div>
        </div>
        <pre id="scanLogBox">No scan yet.</pre>
      </section>

      <section class="card table-card">
        <div class="table-header">
          <h2>Found artifacts</h2>
          <span id="scanTableStatus">Set a root directory and click Scan.</span>
        </div>
        <div class="table-wrap">
          <table>
            <thead>
              <tr>
                <th class="col-sel"></th>
                <th data-sort-header="project"><button class="sort-button" data-sort="project" type="button">Project <span class="sort-indicator"></span></button></th>
                <th class="col-kind" data-sort-header="kind"><button class="sort-button" data-sort="kind" type="button">Artifact type <span class="sort-indicator"></span></button></th>
                <th class="col-size" data-sort-header="size"><button class="sort-button" data-sort="size" type="button">Size <span class="sort-indicator"></span></button></th>
                <th class="col-modified" data-sort-header="modified"><button class="sort-button" data-sort="modified" type="button">Last modified <span class="sort-indicator"></span></button></th>
                <th class="col-age" data-sort-header="age"><button class="sort-button" data-sort="age" type="button">Age <span class="sort-indicator"></span></button></th>
              </tr>
            </thead>
            <tbody id="artifactsBody">
              <tr><td colspan="6" class="empty">Set a root directory and click Scan.</td></tr>
            </tbody>
          </table>
        </div>
      </section>
    </main>
  </div>

  <!-- ═══ Tab 3: Search explanation ═══ -->
  <div id="pageSearchInfo" class="tab-page">
    <main class="shell info-page">
      <section class="hero card">
        <div>
          <p class="eyebrow">transparent discovery rules</p>
          <h1>How searches work</h1>
          <p class="subline">This screen describes the exact matching rules and filters used by both cleanup tools. The app does not accept or execute user-provided regular expressions.</p>
        </div>
      </section>

      <section class="info-grid">
        <article class="card info-card">
          <h2>System Cache Cleaner</h2>
          <p>System discovery iterates over a fixed list of target specifications in the Rust backend. Each target is either a path target or a command target.</p>
          <h3>Path targets</h3>
          <ul>
            <li>Paths are literal locations unless they contain <code>*</code>, <code>?</code>, or bracket expressions such as <code>[0-9]</code>.</li>
            <li><code>~</code> is expanded to the current user’s home directory.</li>
            <li><code>$BREW_CACHE</code> is resolved by running <code>brew --cache</code>; the Homebrew fallback path is also checked.</li>
            <li>Rust’s <code>glob</code> matcher expands wildcard paths such as <code>~/Library/Caches/JetBrains/*/tmp</code> and deduplicates matches.</li>
            <li>Missing paths are omitted from normal discovery. The item’s size is calculated recursively with symlinks not followed.</li>
          </ul>
          <h3>Command targets</h3>
          <ul>
            <li>Commands use fixed executables and fixed argument arrays through <code>std::process::Command</code>; there is no shell interpolation.</li>
            <li>Homebrew, Docker, pnpm, Go, and similar commands are probed only when the executable exists in <code>PATH</code>.</li>
            <li>Command cleanup is disabled until <strong>Allow commands</strong> is enabled.</li>
          </ul>
        </article>

        <article class="card info-card">
          <h2>System target paths</h2>
          <p>The current fixed target list searches these locations and command-backed caches:</p>
          <div class="rule-table">
            <div><strong>Homebrew</strong><span><code>$BREW_CACHE</code>, <code>~/Library/Caches/Homebrew</code>, <code>brew cleanup -s</code></span></div>
            <div><strong>JetBrains</strong><span><code>~/Library/Logs/JetBrains</code>, <code>~/Library/Caches/JetBrains/*/tmp</code>, <code>jcef_cache</code>, <code>index</code>, <code>caches</code>, <code>plugins</code>, <code>~/java_error_in_*.hprof</code>, JVM crash logs</span></div>
            <div><strong>Apple dev</strong><span>Xcode <code>DerivedData</code>, SwiftPM caches, Instruments, Simulator logs</span></div>
            <div><strong>AI / LLM</strong><span>Ollama logs/models, Hugging Face, Torch, MLX, LM Studio, ComfyUI</span></div>
            <div><strong>Python</strong><span>pip, uv, Poetry, Conda/Mamba package caches</span></div>
            <div><strong>Node / JS</strong><span>npm, Yarn, pnpm stores and <code>pnpm store prune</code></span></div>
            <div><strong>Rust</strong><span>Cargo registry cache, Git database, and checkouts</span></div>
            <div><strong>Java / Android</strong><span>Gradle build/module caches, wrapper distributions, Maven repository, Android Studio caches</span></div>
            <div><strong>Docker</strong><span>Desktop logs, builder prune, system prune</span></div>
            <div><strong>Go / Ruby / Dart / PHP</strong><span>Tool caches, module/gem caches, Dart/Flutter, Bundler and Composer caches</span></div>
            <div><strong>Cloud / DevOps</strong><span>Terraform plugins, Pulumi, AWS CDK, Helm</span></div>
            <div><strong>macOS</strong><span>User URLSession downloads and unified log archive</span></div>
          </div>
        </article>

        <article class="card info-card">
          <h2>Project Artifact Scanner</h2>
          <p>The selected root is normalized, then the backend walks its directory tree with <code>WalkDir</code>.</p>
          <ul>
            <li>Traversal starts one level below the root, has a maximum depth of <strong>8</strong>, and does not follow symbolic links.</li>
            <li>Source-control, operating-system, and system directories are skipped: <code>.git</code>, <code>.svn</code>, <code>.hg</code>, <code>Library</code>, <code>System</code>, <code>Applications</code>, <code>Volumes</code>, <code>private</code>, <code>usr</code>, <code>bin</code>, <code>var</code>, <code>tmp</code>, and related macOS metadata folders.</li>
            <li>Matching is based on exact directory names such as <code>node_modules</code>, <code>.venv</code>, <code>target</code>, <code>.next</code>, <code>.terraform</code>, and Android <code>build</code>/<code>.cxx</code> folders.</li>
            <li>Some names require project markers: Rust <code>target</code> needs <code>Cargo.toml</code>; Maven/Gradle targets need their build files; Flutter <code>build</code> needs <code>pubspec.yaml</code>.</li>
            <li>Android leftovers require an Android manifest or a Gradle settings file with an <code>app</code> module. This prevents ordinary folders named <code>build</code> from being mislabeled.</li>
            <li>Once an artifact directory matches, discovery does not recurse inside it again. Exact sizes are then measured concurrently by a bounded pool of up to four workers.</li>
          </ul>
        </article>

        <article class="card info-card">
          <h2>Project filters and cleanup</h2>
          <ul>
            <li><strong>Artifact type</strong> is an exact match against the detected label.</li>
            <li><strong>Min size</strong> keeps artifacts at or above 0, 1 MB, 10 MB, 100 MB, or 1 GB.</li>
            <li><strong>Stale</strong> compares last-modified time against 30, 90, 180, or 365 days.</li>
            <li><strong>Search</strong> is not used by the project scanner; its type, size, and age controls filter the completed result set in the frontend.</li>
            <li>Sorting by project, type, size, last modified, or age is performed locally after filtering.</li>
            <li>Selected artifacts are the exact returned paths. Cleanup moves them to Trash by default or permanently deletes them only when that mode is selected and confirmed.</li>
          </ul>
          <p class="info-note">Every size includes files below the matched directory. Permission errors may make a directory appear smaller or unavailable; the scan log reports the result.</p>
        </article>
      </section>
    </main>
  </div>
`;

// ── Shared helpers ────────────────────────────────────────────────────────────

const $ = (id) => document.getElementById(id);

function escapeHtml(s) {
  return String(s ?? '').replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;').replace(/'/g,'&#039;');
}

function formatBytes(bytes) {
  if (!bytes) return '—';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let v = bytes, u = 0;
  while (v >= 1024 && u < units.length - 1) { v /= 1024; u++; }
  return `${v.toFixed(v >= 10 || u === 0 ? 0 : 1)} ${units[u]}`;
}

function formatNum(n) { return new Intl.NumberFormat().format(n || 0); }

function relativeAge(secs) {
  const days = (Date.now() / 1000 - secs) / 86400;
  if (days < 1)   return 'today';
  if (days < 30)  return `${Math.floor(days)}d`;
  if (days < 365) return `${Math.floor(days / 30)}mo`;
  return `${(days / 365).toFixed(1)}y`;
}

function ageClass(secs) {
  const days = (Date.now() / 1000 - secs) / 86400;
  if (days < 30)  return 'age-fresh';
  if (days < 90)  return 'age-recent';
  if (days < 365) return 'age-aging';
  return 'age-stale';
}

// ── Tab switching ─────────────────────────────────────────────────────────────

document.querySelectorAll('.tab-btn').forEach((btn) => {
  btn.addEventListener('click', () => {
    const tab = btn.dataset.tab;
    document.querySelectorAll('.tab-btn').forEach((b) => b.classList.toggle('active', b === btn));
    $('pageCacheCleaner').classList.toggle('active', tab === 'cache');
    $('pageProjectScanner').classList.toggle('active', tab === 'projects');
    $('pageSearchInfo').classList.toggle('active', tab === 'info');
  });
});

// ── Cache Cleaner — wire-up ───────────────────────────────────────────────────

$('discoverBtn').addEventListener('click', discover);
$('dryRunBtn').addEventListener('click', dryRun);
$('cleanBtn').addEventListener('click', cleanSelected);
$('filterInput').addEventListener('input', (e) => { state.filter = e.target.value.toLowerCase(); render(); });
$('riskFilter').addEventListener('change', (e) => { state.riskFilter = e.target.value; render(); });
$('categoryFilter').addEventListener('change', (e) => { state.categoryFilter = e.target.value; render(); });
$('selectDefaults').addEventListener('click', () => selectMode('defaults'));
$('selectLow').addEventListener('click', () => selectMode('low'));
$('selectNone').addEventListener('click', () => { state.selected.clear(); render(); });
$('copyPlan').addEventListener('click', copyLastPlan);
$('treeCloseBtn').addEventListener('click', closeTree);

$('itemsBody').addEventListener('click', (e) => {
  if (e.target.type === 'checkbox') return;
  const row = e.target.closest('tr[data-row-key]');
  if (!row) return;
  const key = row.dataset.rowKey;
  if (state.activeRowKey === key) { closeTree(); return; }
  state.activeRowKey = key;
  updateActiveRowStyle();
  if (!row.dataset.path) {
    state.tree = { loading: false, nodes: [], breadcrumb: [], rootLabel: row.dataset.label };
    $('treeTitle').textContent = row.dataset.label;
    $('treeCrumb').innerHTML = '';
    $('treeBody').innerHTML = `<div class="tree-empty">Command target — no filesystem path.<br><code style="display:inline;color:#94a3b8">${escapeHtml(row.dataset.command || '')}</code></div>`;
    return;
  }
  loadTree(row.dataset.path, row.dataset.label, [{ name: row.dataset.label, path: row.dataset.path }]);
});

// ── Project Scanner — wire-up ─────────────────────────────────────────────────

$('scanBtn').addEventListener('click', runScan);
$('deleteArtifactsBtn').addEventListener('click', deleteArtifacts);
$('chooseScanRoot').addEventListener('click', chooseScanRoot);
$('saveSearchBtn').addEventListener('click', saveCurrentSearch);
$('savedSearchSelect').addEventListener('change', loadSavedSearch);
$('deleteSavedSearchBtn').addEventListener('click', deleteSavedSearch);
$('artifactTypeFilter').addEventListener('change', (e) => { scanState.typeFilter = e.target.value; renderArtifacts(); });
$('artifactMinSize').addEventListener('change', (e) => { scanState.minSize = +e.target.value; renderArtifacts(); });
$('artifactStaleFilter').addEventListener('change', (e) => { scanState.staleDays = +e.target.value; renderArtifacts(); });
$('artifactSortBy').addEventListener('change', (e) => setArtifactSort(e.target.value));
$('artifactsBody').closest('table').querySelector('thead').addEventListener('click', (e) => {
  const button = e.target.closest('[data-sort]');
  if (button) setArtifactSort(button.dataset.sort);
});
$('selectStale').addEventListener('click', () => selectArtifactMode('stale'));
$('selectHuge').addEventListener('click', () => selectArtifactMode('huge'));
$('selectAllVisible').addEventListener('click', () => selectArtifactMode('all'));
$('selectNoneArtifacts').addEventListener('click', () => { scanState.selected.clear(); renderArtifacts(); });

$('artifactsBody').addEventListener('change', (e) => {
  if (e.target.type !== 'checkbox') return;
  if (e.target.checked) scanState.selected.add(e.target.value);
  else scanState.selected.delete(e.target.value);
  updateScanStats();
});
$('artifactsBody').addEventListener('click', (e) => {
  const folder = e.target.closest('.artifact-folder');
  if (!folder) return;
  const checkbox = folder.closest('tr').querySelector('input[type="checkbox"]');
  checkbox.checked = !checkbox.checked;
  checkbox.dispatchEvent(new Event('change', { bubbles: true }));
});

// ── Cache Cleaner — actions ───────────────────────────────────────────────────

async function discover() {
  setBusy(true);
  setLog('Scanning known macOS developer cache locations…');
  try {
    const report = await invoke('discover', { options: { includeMissing: false } });
    state.report = report;
    state.lastPlan = null;
    state.selected = new Set(report.items.filter((i) => i.selectedByDefault).map((i) => i.id));
    updateCategoryOptions(report.items);
    setLog(`Discovery complete. Found ${report.items.length} targets, ${formatBytes(report.totalBytes)} total.`);
    render();
  } catch (err) { setLog(`Discovery error:\n${String(err)}`); }
  finally { setBusy(false); }
}

async function dryRun() {
  if (!state.report) return setLog('Run Discover first.');
  if (state.selected.size === 0) return setLog('No targets selected.');
  setBusy(true);
  try {
    const plan = await invoke('dry_run', { request: buildRequest() });
    state.lastPlan = plan;
    setLog(formatPlan(plan));
    renderWarnings(plan.warnings || []);
  } catch (err) { setLog(`Dry run error:\n${String(err)}`); }
  finally { setBusy(false); }
}

async function cleanSelected() {
  if (!state.report) return setLog('Run Discover first.');
  if (state.selected.size === 0) return setLog('No targets selected.');
  const request = buildRequest();
  if (state.report.items.some((i) => state.selected.has(i.id) && i.risk === 'high') && !request.includeHighRisk)
    return setLog('High-risk target selected but "Include high-risk" is not enabled.');
  const phrase = request.permanent ? 'DELETE' : 'CLEAN';
  const answer = await showPrompt(
    `This will ${request.permanent ? 'permanently delete' : 'move to Trash'} ${state.selected.size} selected target(s). Type ${phrase} to continue.`, phrase);
  if (answer !== phrase) return setLog('Cleanup cancelled.');
  setBusy(true);
  try {
    const plan = await invoke('clean', { request });
    state.lastPlan = plan; setLog(formatPlan(plan)); renderWarnings(plan.warnings || []);
    const refreshed = await invoke('discover', { options: { includeMissing: false } });
    state.report = refreshed; updateCategoryOptions(refreshed.items); render();
  } catch (err) { setLog(`Cleanup error:\n${String(err)}`); }
  finally { setBusy(false); }
}

// ── Project Scanner — actions ─────────────────────────────────────────────────

async function chooseScanRoot() {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: 'Choose a project root to scan',
    });
    if (typeof selected === 'string') {
      $('scanRoot').value = selected;
      $('scanRoot').focus();
    }
  } catch (err) {
    setScanLog(`Folder picker error:\n${String(err)}`);
  }
}

function currentSearchDefinition() {
  return {
    root: $('scanRoot').value.trim() || '~',
    typeFilter: scanState.typeFilter,
    minSize: scanState.minSize,
    staleDays: scanState.staleDays,
    sortBy: scanState.sortBy,
    sortDirection: scanState.sortDirection,
  };
}

function persistSavedSearches() {
  try {
    localStorage.setItem(SAVED_SEARCHES_KEY, JSON.stringify(scanState.savedSearches));
    renderSavedSearchOptions();
    return true;
  } catch (_) {
    $('savedSearchStatus').textContent = 'Could not save this search; local storage is full.';
    return false;
  }
}

function savedResultsSnapshot(report) {
  if (!report) return null;
  return {
    root: report.root,
    artifacts: report.artifacts,
    totalBytes: report.totalBytes,
    artifactCount: report.artifactCount,
    projectCount: report.projectCount,
    generatedAt: report.generatedAt,
  };
}

function updateActiveSavedSearchResults(report) {
  if (!scanState.activeSavedSearchId) return;
  const index = scanState.savedSearches.findIndex((search) => search.id === scanState.activeSavedSearchId);
  if (index < 0) return;
  scanState.savedSearches[index] = {
    ...scanState.savedSearches[index],
    results: savedResultsSnapshot(report),
  };
  persistSavedSearches();
}

function renderSavedSearchOptions() {
  const select = $('savedSearchSelect');
  if (!select) return;
  select.innerHTML = '<option value="">Choose a saved search…</option>' + scanState.savedSearches
    .slice()
    .sort((a, b) => a.name.localeCompare(b.name))
    .map((search) => `<option value="${escapeHtml(search.id)}">${escapeHtml(search.name)}</option>`)
    .join('');
  select.value = scanState.activeSavedSearchId || '';
  $('deleteSavedSearchBtn').disabled = !scanState.activeSavedSearchId;
}

async function saveCurrentSearch() {
  const selected = scanState.savedSearches.find((search) => search.id === scanState.activeSavedSearchId);
  const name = await showTextPrompt('Name this project artifact search', selected?.name || '');
  if (!name) return;

  const existing = scanState.savedSearches.find((search) => search.name.toLowerCase() === name.toLowerCase());
  const search = {
    id: existing?.id || selected?.id || `search-${Date.now()}`,
    name,
    ...currentSearchDefinition(),
    results: savedResultsSnapshot(scanState.report),
  };
  const replacedIds = new Set([search.id]);
  if (selected) replacedIds.add(selected.id);
  scanState.savedSearches = scanState.savedSearches.filter((item) => !replacedIds.has(item.id));
  scanState.savedSearches.push(search);
  scanState.activeSavedSearchId = search.id;
  persistSavedSearches();
  $('savedSearchStatus').textContent = `Saved “${name}”. It will refresh after deletion.`;
}

async function loadSavedSearch() {
  const id = $('savedSearchSelect').value;
  scanState.activeSavedSearchId = id || null;
  renderSavedSearchOptions();
  if (!id) return;
  const search = scanState.savedSearches.find((item) => item.id === id);
  if (!search) return;
  $('scanRoot').value = search.root;
  scanState.typeFilter = search.typeFilter || 'all';
  scanState.minSize = Number(search.minSize) || 0;
  scanState.staleDays = Number(search.staleDays) || 0;
  scanState.sortBy = search.sortBy || 'size';
  scanState.sortDirection = search.sortDirection || 'desc';
  $('artifactMinSize').value = String(scanState.minSize);
  $('artifactStaleFilter').value = String(scanState.staleDays);
  $('artifactSortBy').value = scanState.sortBy;
  updateArtifactTypeOptions(search.results?.artifacts || []);
  if (search.results && Array.isArray(search.results.artifacts)) {
    scanState.report = {
      ...search.results,
      artifacts: search.results.artifacts,
    };
    scanState.selected.clear();
    renderArtifacts();
    $('savedSearchStatus').textContent = `Loaded “${search.name}”. Showing saved results; click Scan to refresh.`;
    setScanLog(`Loaded saved results for “${search.name}”.`);
    return;
  }
  $('savedSearchStatus').textContent = `Loaded “${search.name}”. No saved results yet; scanning…`;
  await runScan();
}

async function deleteSavedSearch() {
  const search = scanState.savedSearches.find((item) => item.id === scanState.activeSavedSearchId);
  if (!search) return;
  const answer = await showPrompt(`Delete saved search “${search.name}”?`, 'DELETE');
  if (answer !== 'DELETE') return;
  scanState.savedSearches = scanState.savedSearches.filter((item) => item.id !== search.id);
  scanState.activeSavedSearchId = null;
  persistSavedSearches();
  $('savedSearchStatus').textContent = 'Saved search deleted.';
}

async function runScan() {
  const root = $('scanRoot').value.trim() || '~';
  scanState.busy = true;
  scanState.acceptLiveBatches = true;
  scanState.selected.clear();
  scanState.report = {
    root,
    artifacts: [],
    totalBytes: 0,
    artifactCount: 0,
    projectCount: 0,
    generatedAt: new Date().toISOString(),
  };
  updateArtifactTypeOptions([]);
  renderArtifacts();
  $('scanBtn').disabled = true;
  showScanProgress('scan');
  setScanLog(`Scanning ${root} for project artifact directories…`);
  try {
    const report = await invoke('scan_projects', {
      options: {
        root,
        minSizeBytes: 0,   // filtering done in JS
        maxDepth: 8,
      },
    });
    if (scanState.liveRenderTimer) {
      clearTimeout(scanState.liveRenderTimer);
      scanState.liveRenderTimer = null;
    }
    scanState.acceptLiveBatches = false;
    scanState.report = report;
    scanState.selected.clear();
    updateArtifactTypeOptions(report.artifacts);
    updateActiveSavedSearchResults(report);
    setScanLog(
      `Scan complete — ${report.artifactCount} artifacts in ${report.projectCount} projects, ` +
      `${formatBytes(report.totalBytes)} total reclaimable.\n` +
      `Root: ${report.root}  ·  Scanned at ${new Date(report.generatedAt).toLocaleString()}`
    );
    renderArtifacts();
    if (scanState.activeSavedSearchId) {
      const saved = scanState.savedSearches.find((search) => search.id === scanState.activeSavedSearchId);
      if (saved) $('savedSearchStatus').textContent = `Loaded “${saved.name}”. Results reflect the latest scan.`;
    }
  } catch (err) { setScanLog(`Scan error:\n${String(err)}`); }
  finally {
    scanState.busy = false;
    scanState.acceptLiveBatches = false;
    $('scanBtn').disabled = false;
    hideScanProgress();
  }
}

async function deleteArtifacts() {
  if (!scanState.report) return setScanLog('Run a scan first.');
  if (scanState.selected.size === 0) return setScanLog('No artifacts selected.');

  const permanent = $('scanDeleteMode').value === 'permanent';
  const phrase = permanent ? 'DELETE' : 'TRASH';
  const answer = await showPrompt(
    `This will ${permanent ? 'permanently delete' : 'move to Trash'} ${scanState.selected.size} artifact director${scanState.selected.size === 1 ? 'y' : 'ies'}. Type ${phrase} to continue.`,
    phrase
  );
  if (answer !== phrase) return setScanLog('Cancelled.');

  $('deleteArtifactsBtn').disabled = true;
  const paths = Array.from(scanState.selected);
  showScanProgress('delete', paths.length);
  setScanLog(`Deleting ${paths.length} artifact directories…`);
  try {
    const freed = await invoke('delete_artifacts', { request: { paths, permanent } });
    setScanLog(`Done. Freed ${formatBytes(freed)}.\nRe-scanning to update results…`);
    scanState.selected.clear();
    await runScan();
  } catch (err) { setScanLog(`Delete error:\n${String(err)}`); }
  finally { $('deleteArtifactsBtn').disabled = false; hideScanProgress(); }
}

function showScanProgress(phase, total = 0) {
  const progress = $('scanProgress');
  const bar = $('scanProgressBar');
  progress.hidden = false;
  progress.dataset.phase = phase;
  bar.classList.toggle('indeterminate', phase === 'scan');
  bar.style.width = phase === 'scan' ? '' : '0%';
  $('scanProgressText').textContent = phase === 'scan' ? 'Searching project folders…' : 'Preparing deletion…';
  $('scanProgressCount').textContent = phase === 'scan' ? '' : `0 / ${total}`;
  $('scanProgressPath').textContent = '';
}

function hideScanProgress() {
  $('scanProgress').hidden = true;
}

function updateScanProgress(payload) {
  if (payload.phase === 'scan' || payload.phase === 'scan-size' || payload.phase === 'scan-complete') {
    const sizing = payload.phase === 'scan-size';
    const total = payload.total || 0;
    const percent = sizing && total ? Math.round((payload.current / total) * 100) : 0;
    $('scanProgressBar').classList.toggle('indeterminate', payload.phase === 'scan');
    $('scanProgressBar').style.width = sizing ? `${percent}%` : '';
    $('scanProgressText').textContent = payload.phase === 'scan-complete'
      ? `Search complete — ${payload.found} artifacts found`
      : sizing
        ? `Measuring artifacts in parallel… ${payload.found} ready`
        : `Searching… ${payload.found} artifacts found`;
    $('scanProgressCount').textContent = sizing
      ? `${payload.current} / ${total} artifacts measured`
      : `${payload.current} folders checked`;
    $('scanProgressPath').textContent = payload.path || '';
    return;
  }
  if (payload.phase === 'delete' || payload.phase === 'delete-size') {
    const total = payload.total || 0;
    const percent = total ? Math.round((payload.current / total) * 100) : 0;
    $('scanProgressText').textContent = payload.phase === 'delete-size'
      ? 'Measuring artifact before deletion…'
      : 'Deleting selected artifacts…';
    $('scanProgressCount').textContent = `${payload.current} / ${total}`;
    $('scanProgressBar').style.width = `${percent}%`;
    $('scanProgressPath').textContent = payload.path || '';
  }
}

function addLiveArtifactBatch(payload) {
  if (!scanState.acceptLiveBatches || !Array.isArray(payload.artifacts)) return;
  scanState.report.root = payload.root;
  scanState.report.artifacts.push(...payload.artifacts);
  scanState.report.artifactCount = scanState.report.artifacts.length;
  scanState.report.totalBytes += payload.artifacts.reduce((sum, artifact) => sum + artifact.sizeBytes, 0);
  scanState.report.projectCount = new Set(
    scanState.report.artifacts.map((artifact) => artifact.projectPath)
  ).size;

  if (!scanState.liveRenderTimer) {
    scanState.liveRenderTimer = setTimeout(() => {
      scanState.liveRenderTimer = null;
      renderArtifacts();
    }, 500);
  }
}

// ── Project Scanner — render ──────────────────────────────────────────────────

function filteredArtifacts() {
  if (!scanState.report) return [];
  const nowSecs = Date.now() / 1000;
  return scanState.report.artifacts
    .filter((a) => {
      if (scanState.typeFilter !== 'all' && a.artifactKind !== scanState.typeFilter) return false;
      if (a.sizeBytes < scanState.minSize) return false;
      if (scanState.staleDays > 0) {
        const ageDays = (nowSecs - a.modifiedSecs) / 86400;
        if (ageDays < scanState.staleDays) return false;
      }
      return true;
    })
    .sort((a, b) => {
      let result;
      if (scanState.sortBy === 'size') result = a.sizeBytes - b.sizeBytes;
      else if (scanState.sortBy === 'modified') result = a.modifiedSecs - b.modifiedSecs;
      else if (scanState.sortBy === 'age') result = b.modifiedSecs - a.modifiedSecs;
      else if (scanState.sortBy === 'project') result = a.projectName.localeCompare(b.projectName);
      else if (scanState.sortBy === 'kind') result = a.artifactKind.localeCompare(b.artifactKind);
      else result = 0;
      if (result === 0) result = a.path.localeCompare(b.path);
      return scanState.sortDirection === 'asc' ? result : -result;
    });
}

function setArtifactSort(sortBy) {
  if (scanState.sortBy === sortBy) {
    scanState.sortDirection = scanState.sortDirection === 'asc' ? 'desc' : 'asc';
  } else {
    scanState.sortBy = sortBy;
    scanState.sortDirection = sortBy === 'size' || sortBy === 'modified' || sortBy === 'age' ? 'desc' : 'asc';
  }
  $('artifactSortBy').value = sortBy;
  renderArtifacts();
}

function updateArtifactSortIndicators() {
  document.querySelectorAll('[data-sort-header]').forEach((header) => {
    const button = header.querySelector('[data-sort]');
    const active = button.dataset.sort === scanState.sortBy;
    header.setAttribute('aria-sort', active
      ? (scanState.sortDirection === 'asc' ? 'ascending' : 'descending')
      : 'none');
    button.classList.toggle('active', active);
    button.querySelector('.sort-indicator').textContent = active
      ? (scanState.sortDirection === 'asc' ? ' ↑' : ' ↓')
      : '';
  });
}

function renderArtifacts() {
  const items = filteredArtifacts();
  const body = $('artifactsBody');

  if (!scanState.report) {
    body.innerHTML = `<tr><td colspan="6" class="empty">Set a root directory and click Scan.</td></tr>`;
    updateScanStats();
    return;
  }
  if (items.length === 0) {
    body.innerHTML = `<tr><td colspan="6" class="empty">No artifacts match the current filters.</td></tr>`;
    updateScanStats();
    return;
  }

  body.innerHTML = items.map(renderArtifactRow).join('');
  updateArtifactSortIndicators();
  updateScanStats();
}

function renderArtifactRow(artifact) {
  const checked = scanState.selected.has(artifact.path) ? 'checked' : '';
  const relativePath = relativeArtifactPath(artifact.path, scanState.report.root);
  const modDate = artifact.modifiedSecs
    ? new Date(artifact.modifiedSecs * 1000).toLocaleDateString('en-US', { year: 'numeric', month: 'short', day: 'numeric' })
    : '—';
  const ageStr = artifact.modifiedSecs ? relativeAge(artifact.modifiedSecs) : '—';
  const cls    = artifact.modifiedSecs ? ageClass(artifact.modifiedSecs) : '';
  return `
    <tr>
      <td class="col-sel"><input type="checkbox" value="${escapeHtml(artifact.path)}" ${checked} /></td>
      <td class="artifact-folder" title="Click to ${checked ? 'deselect' : 'select'} this folder">
        <strong>${escapeHtml(artifact.projectName)}</strong>
        <div class="note" title="${escapeHtml(artifact.path)}"><span style="color:#60a5fa">${escapeHtml(relativePath)}</span></div>
      </td>
      <td class="col-kind"><span class="kind-chip">${escapeHtml(artifact.artifactKind)}</span></td>
      <td class="col-size">${formatBytes(artifact.sizeBytes)}</td>
      <td class="col-modified">${modDate}</td>
      <td class="col-age"><span class="age-chip ${cls}">${ageStr}</span></td>
    </tr>`;
}

function relativeArtifactPath(path, root) {
  const normalizedRoot = root.replace(/[\\/]+$/, '');
  if (path === normalizedRoot) return '.';
  const prefix = `${normalizedRoot}/`;
  return path.startsWith(prefix) ? path.slice(prefix.length) : path;
}

function updateScanStats() {
  const items = filteredArtifacts();
  const total = scanState.report?.totalBytes ?? 0;
  const artifacts = scanState.report?.artifactCount ?? 0;
  const projects  = scanState.report?.projectCount ?? 0;
  const selectedSize = scanState.report
    ? scanState.report.artifacts
      .filter((artifact) => scanState.selected.has(artifact.path))
      .reduce((sum, artifact) => sum + artifact.sizeBytes, 0)
    : 0;
  $('scanTotalSize').textContent = total ? formatBytes(total) : '—';
  $('scanArtifactCount').textContent = artifacts || '—';
  $('scanProjectCount').textContent = projects || '—';
  $('scanVisibleCount').textContent = scanState.report
    ? `${items.length} / ${scanState.selected.size} sel`
    : '—';
  $('scanSelectedSize').textContent = scanState.report ? formatBytes(selectedSize) : '—';
  $('scanTableStatus').textContent = scanState.report
    ? scanState.busy
      ? `${artifacts} artifacts found so far · ${items.length} visible`
      : `${items.length} of ${artifacts} artifacts visible`
    : 'Set a root directory and click Scan.';
}

function updateArtifactTypeOptions(artifacts) {
  const kinds = Array.from(new Set(artifacts.map((a) => a.artifactKind))).sort();
  $('artifactTypeFilter').innerHTML =
    `<option value="all">All types</option>` +
    kinds.map((k) => `<option value="${escapeHtml(k)}">${escapeHtml(k)}</option>`).join('');
  if (artifacts.length === 0) return;
  if (scanState.typeFilter !== 'all' && kinds.includes(scanState.typeFilter)) {
    $('artifactTypeFilter').value = scanState.typeFilter;
  } else if (scanState.typeFilter !== 'all') {
    scanState.typeFilter = 'all';
  }
}

function selectArtifactMode(mode) {
  if (!scanState.report) return;
  const nowSecs = Date.now() / 1000;
  const items = filteredArtifacts();
  scanState.selected.clear();
  for (const a of items) {
    if (mode === 'all') { scanState.selected.add(a.path); continue; }
    if (mode === 'stale') {
      const days = (nowSecs - a.modifiedSecs) / 86400;
      if (days >= 365) scanState.selected.add(a.path);
    }
    if (mode === 'huge' && a.sizeBytes >= 500 * 1024 * 1024) {
      scanState.selected.add(a.path);
    }
  }
  renderArtifacts();
}

function setScanLog(text) { $('scanLogBox').textContent = text; }

// ── Cache Cleaner — tree viewer ───────────────────────────────────────────────

async function loadTree(path, rootLabel, breadcrumb) {
  state.tree = { loading: true, nodes: [], breadcrumb, rootLabel };
  $('treeTitle').textContent = rootLabel;
  renderBreadcrumb();
  $('treeBody').innerHTML = `<div class="tree-loading"><div class="spinner"></div>Computing sizes…</div>`;
  try {
    const nodes = await invoke('list_path', { path });
    state.tree = { loading: false, nodes, breadcrumb, rootLabel };
    renderTreeBody();
  } catch (err) {
    $('treeBody').innerHTML = `<div class="tree-empty">Error: ${escapeHtml(String(err))}</div>`;
  }
}

function navigateTree(name, path) {
  loadTree(path, state.tree.rootLabel, [...state.tree.breadcrumb, { name, path }]);
}

function navigateCrumb(index) {
  const seg = state.tree.breadcrumb[index];
  loadTree(seg.path, state.tree.rootLabel, state.tree.breadcrumb.slice(0, index + 1));
}

function closeTree() {
  state.activeRowKey = null;
  state.tree = { loading: false, nodes: [], breadcrumb: [], rootLabel: '' };
  updateActiveRowStyle();
  $('treeTitle').textContent = 'Contents';
  $('treeCrumb').innerHTML = '';
  $('treeBody').innerHTML = `<div class="tree-empty">Click any row to browse<br>its filesystem contents.</div>`;
}

function renderBreadcrumb() {
  const parts = state.tree.breadcrumb;
  if (parts.length <= 1) { $('treeCrumb').innerHTML = ''; return; }
  $('treeCrumb').innerHTML = parts.map((seg, i) =>
    i === parts.length - 1
      ? `<span class="crumb-cur">${escapeHtml(seg.name)}</span>`
      : `<span class="crumb-seg" data-index="${i}">${escapeHtml(seg.name)}</span><span class="crumb-sep">/</span>`
  ).join('');
  $('treeCrumb').querySelectorAll('.crumb-seg').forEach((el) =>
    el.addEventListener('click', () => navigateCrumb(+el.dataset.index))
  );
}

function renderTreeBody() {
  const { nodes } = state.tree;
  if (nodes.length === 0) { $('treeBody').innerHTML = '<div class="tree-empty">Empty or unreadable.</div>'; return; }
  const maxSize = Math.max(...nodes.map((n) => n.sizeBytes), 1);
  $('treeBody').innerHTML = nodes.map((node) => {
    const pct = node.sizeBytes ? Math.max(2, Math.round((node.sizeBytes / maxSize) * 100)) : 0;
    return `<div class="tree-node ${node.isDir ? 'is-dir' : 'is-file'}"
        data-path="${escapeHtml(node.path)}" data-name="${escapeHtml(node.name)}" title="${escapeHtml(node.path)}">
      <span class="tree-icon">${node.isDir ? '▸' : '·'}</span>
      <span class="tree-name">${escapeHtml(node.name)}</span>
      <div class="tree-bar-track"><div class="tree-bar-fill" style="width:${pct}%"></div></div>
      <span class="tree-size">${formatBytes(node.sizeBytes)}</span>
      <span class="tree-count">${node.isDir ? formatNum(node.entryCount) + ' items' : ''}</span>
    </div>`;
  }).join('');
  $('treeBody').querySelectorAll('.tree-node.is-dir').forEach((el) =>
    el.addEventListener('click', () => navigateTree(el.dataset.name, el.dataset.path))
  );
  renderBreadcrumb();
}

function updateActiveRowStyle() {
  document.querySelectorAll('tr[data-row-key]').forEach((row) =>
    row.classList.toggle('row-active', row.dataset.rowKey === state.activeRowKey)
  );
}

// ── Cache Cleaner — render ────────────────────────────────────────────────────

function render() {
  const items = filteredItems();
  if (!state.report) {
    $('itemsBody').innerHTML = `<tr><td colspan="6" class="empty">Click Discover to scan known developer cache locations.</td></tr>`;
    renderStats([]);
    return;
  }
  if (items.length === 0) {
    $('itemsBody').innerHTML = `<tr><td colspan="6" class="empty">No targets match the current filters.</td></tr>`;
  } else {
    $('itemsBody').innerHTML = items.map(renderRow).join('');
    $('itemsBody').querySelectorAll('input[type="checkbox"]').forEach((cb) => {
      cb.addEventListener('change', (e) => {
        if (e.target.checked) state.selected.add(e.target.value);
        else state.selected.delete(e.target.value);
        renderStats(filteredItems());
      });
      cb.addEventListener('click', (e) => e.stopPropagation());
    });
    updateActiveRowStyle();
  }
  renderStats(items);
  renderWarnings(state.report.warnings || []);
}

function filteredItems() {
  if (!state.report) return [];
  const q = state.filter;
  return state.report.items.filter((item) => {
    const hay = `${item.label} ${item.category} ${item.path || ''} ${item.command || ''} ${item.note || ''}`.toLowerCase();
    return (!q || hay.includes(q))
      && (state.riskFilter === 'all' || item.risk === state.riskFilter)
      && (state.categoryFilter === 'all' || item.category === state.categoryFilter);
  });
}

function renderRow(item) {
  const checked = state.selected.has(item.id) ? 'checked' : '';
  const disabled = item.exists ? '' : 'disabled';
  const warning = item.warning ? `<div class="row-warning">${escapeHtml(item.warning)}</div>` : '';
  const rowKey = `${item.id}::${item.path || item.command || ''}`;
  return `
    <tr data-row-key="${escapeHtml(rowKey)}" data-path="${escapeHtml(item.path || '')}"
        data-command="${escapeHtml(item.command || '')}" data-label="${escapeHtml(item.label)}" class="risk-${item.risk}">
      <td class="col-sel"><input type="checkbox" value="${escapeHtml(item.id)}" ${checked} ${disabled} /></td>
      <td><strong>${escapeHtml(item.label)}</strong><div class="note">${escapeHtml(item.note)}</div>${warning}</td>
      <td class="col-cat">${escapeHtml(item.category)}</td>
      <td class="col-risk"><span class="chip ${item.risk}">${item.risk}</span></td>
      <td class="col-size">${formatBytes(item.sizeBytes)}</td>
      <td class="col-path"><code>${escapeHtml(item.path || item.command || '—')}</code></td>
    </tr>`;
}

function renderStats(visibleItems) {
  const all = state.report?.items || [];
  const sel = all.filter((i) => state.selected.has(i.id));
  $('totalSize').textContent = state.report ? formatBytes(state.report.totalBytes) : '—';
  $('selectedSize').textContent = formatBytes(sel.reduce((s, i) => s + i.sizeBytes, 0));
  $('itemCount').textContent = state.report ? String(all.length) : '—';
  $('visibleCount').textContent = state.report ? `${visibleItems.length} visible / ${all.length} found` : 'No scan yet';
  $('lastScan').textContent = state.report ? new Date(state.report.generatedAt).toLocaleString() : '—';
}

function renderWarnings(warnings) {
  $('warnings').innerHTML = (warnings || []).map((w) => `<div class="warn">${escapeHtml(w)}</div>`).join('');
}

function updateCategoryOptions(items) {
  const cats = Array.from(new Set(items.map((i) => i.category))).sort();
  $('categoryFilter').innerHTML = `<option value="all">All</option>` +
    cats.map((c) => `<option value="${escapeHtml(c)}">${escapeHtml(c)}</option>`).join('');
}

function selectMode(mode) {
  if (!state.report) return;
  state.selected.clear();
  for (const item of state.report.items) {
    if (mode === 'defaults' && item.selectedByDefault) state.selected.add(item.id);
    if (mode === 'low'      && item.risk === 'low')    state.selected.add(item.id);
  }
  render();
}

function buildRequest() {
  return {
    selectedIds: Array.from(state.selected),
    permanent: !$('moveToTrash').checked,
    allowCommands: $('allowCommands').checked,
    includeHighRisk: $('includeHighRisk').checked,
  };
}

function formatPlan(plan) {
  const lines = [
    `${plan.dryRun ? 'DRY RUN' : 'CLEANUP'} @ ${new Date(plan.generatedAt).toLocaleString()}`,
    `Mode: ${plan.permanent ? 'permanent delete' : 'move to Trash'}`,
    `Affected: ${formatBytes(plan.totalBytes)}`, '',
  ];
  for (const a of plan.actions) {
    lines.push(`- [${a.status}] ${a.label} (${a.risk}, ${a.category})`);
    if (a.path)    lines.push(`  path: ${a.path}`);
    if (a.command) lines.push(`  command: ${a.command}`);
    if (a.sizeBytes) lines.push(`  size: ${formatBytes(a.sizeBytes)}`);
    if (a.output) {
      const out = a.output.length > 1200 ? `${a.output.slice(0, 1200)}…` : a.output;
      lines.push(...String(out).split('\n').map((l, i) => `${i === 0 ? '  output: ' : '          '}${l}`));
    }
  }
  if (plan.warnings?.length) { lines.push('', 'Warnings:'); plan.warnings.forEach((w) => lines.push(`- ${w}`)); }
  return lines.join('\n');
}

async function copyLastPlan() {
  if (!state.lastPlan) return setLog('No dry-run or cleanup plan to copy yet.');
  await navigator.clipboard.writeText(formatPlan(state.lastPlan));
  setLog('Copied last plan to clipboard.');
}

function setBusy(b) {
  state.busy = b;
  ['discoverBtn', 'dryRunBtn', 'cleanBtn'].forEach((id) => $(id).disabled = b);
}

function setLog(text) { $('logBox').textContent = text; }

let memoryPollBusy = false;
async function updateMemoryUsage() {
  if (memoryPollBusy) return;
  memoryPollBusy = true;
  try {
    const bytes = await invoke('memory_usage');
    $('memoryUsage').textContent = formatBytes(bytes);
  } catch (_) {
    $('memoryUsage').textContent = 'unavailable';
  } finally {
    memoryPollBusy = false;
  }
}

// ── Init ──────────────────────────────────────────────────────────────────────

render();
updateScanStats();
updateArtifactSortIndicators();
renderSavedSearchOptions();
listen('project-progress', (event) => updateScanProgress(event.payload));
listen('project-artifact-batch', (event) => addLiveArtifactBatch(event.payload));
updateMemoryUsage();
setInterval(updateMemoryUsage, 3000);
