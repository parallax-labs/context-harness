/**
 * ctx-search.js — Drop-in search widget for Context Harness knowledge bases.
 *
 * Usage:
 *   <script src="ctx-search.js" data-json="data.json"></script>
 *
 * Or programmatic:
 *   <script src="ctx-search.js"></script>
 *   <script>CtxSearch.init({ dataUrl: 'data.json' })</script>
 *
 * Options (via data attributes or init()):
 *   dataUrl       — path to the data.json file (docs/repo index)
 *   siteIndexUrl  — path to site-index.json for site navigation (optional)
 *   placeholder   — search input placeholder text
 *   hotkey        — keyboard shortcut letter (default: 'k' for Cmd+K)
 *   trigger       — CSS selector for an existing element to use as trigger
 *
 * Generates data.json:
 *   ctx init --config ctx.toml
 *   ctx sync filesystem --config ctx.toml
 *   # export with build-docs.sh or a simple sqlite3/python script
 */
(function () {
  'use strict';

  // ── BM25 engine ──
  const K1 = 1.2, B = 0.75;

  function tokenize(text) {
    return text.toLowerCase().replace(/[^a-z0-9\s]/g, ' ').split(/\s+/).filter(t => t.length > 1);
  }

  function buildIndex(chunks) {
    const N = chunks.length, df = {}, tfs = [], lens = [];
    for (const c of chunks) {
      const tokens = tokenize(c.text);
      lens.push(tokens.length);
      const tf = {}, seen = new Set();
      for (const t of tokens) {
        tf[t] = (tf[t] || 0) + 1;
        if (!seen.has(t)) { df[t] = (df[t] || 0) + 1; seen.add(t); }
      }
      tfs.push(tf);
    }
    const avgdl = lens.reduce((a, b) => a + b, 0) / N;
    const idf = {};
    for (const [t, n] of Object.entries(df)) idf[t] = Math.log((N - n + 0.5) / (n + 0.5) + 1);
    return { idf, avgdl, tfs, lens, N };
  }

  function search(query, data, index, limit) {
    const qTokens = tokenize(query);
    if (!qTokens.length) return [];
    const scored = [];
    for (let i = 0; i < data.chunks.length; i++) {
      let s = 0;
      for (const q of qTokens) {
        const tf = index.tfs[i][q] || 0;
        if (!tf) continue;
        s += (index.idf[q] || 0) * ((tf * (K1 + 1)) / (tf + K1 * (1 - B + B * index.lens[i] / index.avgdl)));
      }
      if (s > 0) scored.push({ ci: i, s });
    }
    scored.sort((a, b) => b.s - a.s);

    // Group by document (MAX)
    const docMap = {};
    for (const { ci, s } of scored) {
      const did = data.chunks[ci].document_id;
      if (!docMap[did] || s > docMap[did].s) docMap[did] = { did, s, ci };
    }
    const results = Object.values(docMap).sort((a, b) => b.s - a.s).slice(0, limit);

    // Normalize scores
    const maxS = results.length ? results[0].s : 1;
    return results.map(r => {
      const doc = data.documents.find(d => d.id === r.did);
      const chunk = data.chunks[r.ci];
      return {
        title: doc?.title || doc?.source_id || 'Untitled',
        source: doc?.source_id || '',
        url: doc?.source_url || null,
        score: maxS > 0 ? r.s / maxS : 0,
        snippet: chunk.text.substring(0, 240) + (chunk.text.length > 240 ? '…' : ''),
      };
    });
  }

  function searchSitePages(siteIndex, query, limit) {
    if (!siteIndex || !Array.isArray(siteIndex)) return [];
    const qTokens = tokenize(query);
    if (!qTokens.length) return [];
    const scored = [];
    const text = (query || '').toLowerCase();
    for (const page of siteIndex) {
      const title = (page.title || '').toLowerCase();
      const desc = (page.description || '').toLowerCase();
      const combined = title + ' ' + desc;
      let s = 0;
      for (const t of qTokens) {
        if (title.includes(t)) s += 1.5;
        if (desc.includes(t)) s += 0.5;
        if (combined.includes(t)) s += 0.3;
      }
      if (s > 0) scored.push({ page, s });
    }
    scored.sort((a, b) => b.s - a.s);
    const maxS = scored.length ? scored[0].s : 1;
    return scored.slice(0, limit).map(({ page, s }) => ({
      title: page.title,
      url: page.url,
      source: 'Page',
      score: maxS > 0 ? Math.min(1, s / maxS) : 0,
      snippet: page.description || '',
      type: 'site',
    }));
  }

  function isSameOrigin(url) {
    if (!url) return false;
    try {
      return new URL(url, location.href).origin === location.origin;
    } catch (_) { return false; }
  }

  // ── Styles ──
  const CSS = `
    .ctx-search-trigger{display:inline-flex;align-items:center;gap:8px;padding:7px 14px;
      background:rgba(255,255,255,0.04);border:1px solid rgba(255,255,255,0.1);border-radius:8px;
      color:rgba(255,255,255,0.5);font-size:13px;cursor:pointer;transition:all .15s;font-family:inherit}
    .ctx-search-trigger:hover{border-color:rgba(255,255,255,0.2);color:rgba(255,255,255,0.7);
      background:rgba(255,255,255,0.06)}
    .ctx-search-trigger kbd{padding:2px 6px;border-radius:4px;font-size:11px;font-family:inherit;
      background:rgba(255,255,255,0.06);border:1px solid rgba(255,255,255,0.1)}
    .ctx-overlay{position:fixed;inset:0;z-index:10000;background:rgba(0,0,0,0.6);
      backdrop-filter:blur(4px);display:none;align-items:flex-start;justify-content:center;padding:min(12vh,120px) 24px 24px}
    .ctx-overlay.open{display:flex}
    .ctx-modal{width:100%;max-width:620px;background:#12121a;border:1px solid #1e1e2e;
      border-radius:12px;box-shadow:0 24px 80px rgba(0,0,0,0.6);overflow:hidden;
      display:flex;flex-direction:column;max-height:70vh}
    .ctx-input-wrap{display:flex;align-items:center;padding:0 16px;border-bottom:1px solid #1e1e2e}
    .ctx-input-wrap svg{flex-shrink:0;opacity:0.4}
    .ctx-input{flex:1;padding:16px 12px;background:none;border:none;color:#e4e4ec;font-size:16px;
      outline:none;font-family:inherit}
    .ctx-input::placeholder{color:#55556a}
    .ctx-results{overflow-y:auto;padding:8px;flex:1}
    .ctx-result{display:block;padding:10px 14px;border-radius:8px;cursor:pointer;transition:background .1s;
      text-decoration:none;color:inherit}
    .ctx-result:hover,.ctx-result.selected{background:rgba(79,143,255,0.08)}
    .ctx-result-title{font-size:14px;font-weight:600;color:#e4e4ec;margin-bottom:2px;
      display:flex;align-items:center;justify-content:space-between}
    .ctx-result-score{font-size:11px;font-family:'SF Mono','Fira Code',monospace;
      padding:1px 7px;border-radius:10px;background:rgba(74,222,128,0.12);color:#4ade80;flex-shrink:0}
    .ctx-result-source{font-size:12px;color:#55556a;margin-bottom:4px;
      font-family:'SF Mono','Fira Code',monospace}
    .ctx-result-snippet{font-size:13px;color:#8888a0;line-height:1.5}
    .ctx-empty{padding:32px 16px;text-align:center;color:#55556a;font-size:14px}
    .ctx-footer{padding:8px 16px;border-top:1px solid #1e1e2e;display:flex;align-items:center;
      justify-content:space-between;font-size:11px;color:#55556a}
    .ctx-footer kbd{padding:1px 5px;border-radius:3px;background:rgba(255,255,255,0.06);
      border:1px solid rgba(255,255,255,0.1);font-family:inherit;font-size:10px;margin:0 2px}
  `;

  // ── Widget ──
  function createWidget(opts) {
    // Inject styles
    const style = document.createElement('style');
    style.textContent = CSS;
    document.head.appendChild(style);

    // State
    let data = null, index = null, siteIndex = null, selectedIdx = -1, results = [];

    // Load doc index
    if (opts.dataUrl) {
      fetch(opts.dataUrl)
        .then(r => r.json())
        .then(d => {
          data = d;
          index = d.chunks ? buildIndex(d.chunks) : null;
          if (triggerEl) {
            const count = (d.documents ? d.documents.length : 0) + (siteIndex ? siteIndex.length : 0);
            triggerEl.title = `Search site and docs (${opts.hotkey === 'k' ? '⌘K' : '⌘' + opts.hotkey.toUpperCase()})`;
          }
        })
        .catch(e => console.warn('[ctx-search] Failed to load data:', e));
    }

    // Load site index (pages for navigation)
    if (opts.siteIndexUrl) {
      fetch(opts.siteIndexUrl)
        .then(r => r.json())
        .then(d => {
          siteIndex = Array.isArray(d) ? d : [];
          if (triggerEl && !opts.dataUrl) {
            triggerEl.title = `Search ${siteIndex.length} pages (${opts.hotkey === 'k' ? '⌘K' : '⌘' + opts.hotkey.toUpperCase()})`;
          }
        })
        .catch(e => console.warn('[ctx-search] Failed to load site index:', e));
    }

    // Create trigger button (unless user provides one)
    let triggerEl;
    if (opts.trigger) {
      triggerEl = document.querySelector(opts.trigger);
    }
    if (!triggerEl) {
      triggerEl = document.createElement('button');
      triggerEl.className = 'ctx-search-trigger';
      triggerEl.innerHTML = `<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.35-4.35"/></svg>Search<kbd>${navigator.platform.includes('Mac') ? '⌘' : 'Ctrl+'}${opts.hotkey.toUpperCase()}</kbd>`;
      // Insert into sidebar header or body
      const sidebar = document.querySelector('.sidebar');
      if (sidebar) {
        triggerEl.style.margin = '0 16px 16px';
        triggerEl.style.width = 'calc(100% - 32px)';
        sidebar.insertBefore(triggerEl, sidebar.firstChild);
      } else {
        triggerEl.style.position = 'fixed';
        triggerEl.style.top = '12px';
        triggerEl.style.right = '12px';
        triggerEl.style.zIndex = '9999';
        document.body.appendChild(triggerEl);
      }
    }

    // Create overlay + modal
    const overlay = document.createElement('div');
    overlay.className = 'ctx-overlay';
    overlay.innerHTML = `
      <div class="ctx-modal">
        <div class="ctx-input-wrap">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.35-4.35"/></svg>
          <input class="ctx-input" type="text" placeholder="${opts.placeholder}" autocomplete="off" spellcheck="false">
        </div>
        <div class="ctx-results"></div>
        <div class="ctx-footer">
          <span><kbd>↑↓</kbd> navigate <kbd>↵</kbd> open <kbd>esc</kbd> close</span>
          <span>powered by Context Harness</span>
        </div>
      </div>
    `;
    document.body.appendChild(overlay);

    const input = overlay.querySelector('.ctx-input');
    const resultsEl = overlay.querySelector('.ctx-results');

    function open() {
      overlay.classList.add('open');
      input.value = '';
      input.focus();
      resultsEl.innerHTML = '<div class="ctx-empty">Type to search…</div>';
      selectedIdx = -1;
      results = [];
    }

    function close() {
      overlay.classList.remove('open');
      input.value = '';
    }

    function render() {
      if (!results.length) {
        resultsEl.innerHTML = input.value.trim()
          ? '<div class="ctx-empty">No results found</div>'
          : '<div class="ctx-empty">Type to search…</div>';
        return;
      }
      const targetAttr = (r) => r.url && isSameOrigin(r.url) ? '' : (r.url ? ' target="_blank" rel="noopener"' : '');
      resultsEl.innerHTML = results.map((r, i) => `
        <a class="ctx-result${i === selectedIdx ? ' selected' : ''}" ${r.url ? `href="${esc(r.url)}"${targetAttr(r)}` : 'href="#"'} data-idx="${i}">
          <div class="ctx-result-title">
            <span>${esc(r.title)}</span>
            <span class="ctx-result-score">${r.type === 'site' ? 'Page' : r.score.toFixed(2)}</span>
          </div>
          <div class="ctx-result-source">${esc(r.source)}</div>
          ${r.snippet ? `<div class="ctx-result-snippet">${esc(r.snippet)}</div>` : ''}
        </a>
      `).join('');
    }

    function navigate(dir) {
      if (!results.length) return;
      selectedIdx = Math.max(-1, Math.min(results.length - 1, selectedIdx + dir));
      render();
      const sel = resultsEl.querySelector('.selected');
      if (sel) sel.scrollIntoView({ block: 'nearest' });
    }

    function openSelected() {
      if (selectedIdx >= 0 && results[selectedIdx]) {
        const r = results[selectedIdx];
        if (r.url) {
          if (isSameOrigin(r.url)) {
            window.location.href = r.url;
          } else {
            window.open(r.url, '_blank');
          }
        }
        close();
      }
    }

    // Events
    triggerEl.addEventListener('click', open);

    overlay.addEventListener('click', (e) => {
      if (e.target === overlay) close();
    });

    input.addEventListener('input', () => {
      const q = input.value.trim();
      if (!q) {
        results = [];
        selectedIdx = -1;
        render();
        return;
      }
      const siteResults = searchSitePages(siteIndex, q, 6);
      const docResults = (data && index) ? search(q, data, index, 8) : [];
      results = [...siteResults, ...docResults].slice(0, 14);
      selectedIdx = results.length ? 0 : -1;
      render();
    });

    input.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') { close(); e.preventDefault(); }
      else if (e.key === 'ArrowDown') { navigate(1); e.preventDefault(); }
      else if (e.key === 'ArrowUp') { navigate(-1); e.preventDefault(); }
      else if (e.key === 'Enter') { openSelected(); e.preventDefault(); }
    });

    document.addEventListener('keydown', (e) => {
      const isMac = navigator.platform.includes('Mac');
      const mod = isMac ? e.metaKey : e.ctrlKey;
      if (mod && e.key.toLowerCase() === opts.hotkey) {
        e.preventDefault();
        if (overlay.classList.contains('open')) close(); else open();
      }
    });

    resultsEl.addEventListener('click', (e) => {
      const link = e.target.closest('.ctx-result');
      if (link) {
        if (link.getAttribute('href') === '#') e.preventDefault();
        close();
      }
    });
  }

  function esc(s) {
    const d = document.createElement('div');
    d.textContent = s;
    return d.innerHTML;
  }

  // ── Public API ──
  const CtxSearch = {
    init(opts = {}) {
      const defaults = { dataUrl: 'data.json', siteIndexUrl: '', placeholder: 'Search docs and site…', hotkey: 'k', trigger: null };
      createWidget({ ...defaults, ...opts });
    }
  };

  // Auto-init from script tag data attributes
  const script = document.currentScript;
  if (script && (script.dataset.json || script.dataset.siteIndex)) {
    const init = () => {
      CtxSearch.init({
        dataUrl: script.dataset.json || null,
        siteIndexUrl: script.dataset.siteIndex || null,
        placeholder: script.dataset.placeholder || 'Search docs and site…',
        hotkey: script.dataset.hotkey || 'k',
        trigger: script.dataset.trigger || null,
      });
    };
    if (document.readyState === 'loading') {
      document.addEventListener('DOMContentLoaded', init);
    } else {
      init();
    }
  }

  window.CtxSearch = CtxSearch;
})();

