// RustIO search-page client. Vanilla JS, no framework.
//
// Contract with the server:
//   GET /search?<params>&format=json
//     → { q, hits, total, ms, page, page_size, total_pages,
//         sort, filters, facets }
(function () {
    'use strict';

    const DEBOUNCE_MS = 150;

    const page = document.querySelector('.search-page');
    if (!page) return;

    const input    = document.getElementById('search-input');
    const clearBtn = document.getElementById('search-clear');
    const sortSel  = document.getElementById('search-sort');
    const sidebar  = document.getElementById('search-sidebar');
    const chipsEl  = document.getElementById('search-chips');
    const results  = document.getElementById('search-results');
    const countEl  = document.getElementById('search-count');
    const bar      = page.querySelector('.search-bar');
    const hrefTpl  = page.dataset.hrefTemplate || '/admin/posts/{id}/edit';

    let state;
    try { state = JSON.parse(page.dataset.initial || '{}'); }
    catch (e) { console.warn('search: bad data-initial', e); state = {}; }
    state.filters = state.filters || {};
    state.filters.author = state.filters.author || [];

    let debounceTimer = null;
    let pending = null;          // AbortController of the in-flight fetch
    let selectedIndex = -1;

    // --- fetch + render -----------------------------------------------------

    function schedule() {
        if (debounceTimer) clearTimeout(debounceTimer);
        results.classList.add('is-loading');
        debounceTimer = setTimeout(run, DEBOUNCE_MS);
    }

    async function run() {
        if (pending) pending.abort();
        pending = new AbortController();
        const params = buildParams();
        history.replaceState(null, '', '/search' + (params.toString() ? '?' + params : ''));
        try {
            const resp = await fetch('/search?' + params + '&format=json',
                { signal: pending.signal, headers: { 'accept': 'application/json' } });
            if (!resp.ok) throw new Error('HTTP ' + resp.status);
            state = await resp.json();
            state.filters = state.filters || {};
            state.filters.author = state.filters.author || [];
            render();
        } catch (err) {
            if (err.name !== 'AbortError') console.warn('search failed', err);
        } finally {
            results.classList.remove('is-loading');
            pending = null;
        }
    }

    function buildParams() {
        const p = new URLSearchParams();
        if (state.q) p.set('q', state.q);
        if (state.filters.published === true)  p.set('published', 'true');
        if (state.filters.published === false) p.set('published', 'false');
        if (state.filters.author.length) p.set('author', state.filters.author.join(','));
        if (state.filters.date_range) p.set('date_range', state.filters.date_range);
        if (state.sort && state.sort !== 'relevance') p.set('sort', state.sort);
        if (state.page && state.page > 1) p.set('page', String(state.page));
        return p;
    }

    function render() {
        const n = state.total || 0;
        if (countEl) countEl.textContent = n + ' result' + (n === 1 ? '' : 's') + ' in ' + (state.ms || 0) + 'ms';
        renderChips();
        renderFacets();
        renderResults();
        selectedIndex = -1;
    }

    function renderChips() {
        const frags = [];
        if (state.filters.published === true)  frags.push(chip('published', 'true',  'Published'));
        if (state.filters.published === false) frags.push(chip('published', 'false', 'Draft'));
        for (const a of state.filters.author) frags.push(chip('author', a, a));
        if (state.filters.date_range) frags.push(chip('date_range', state.filters.date_range, 'Last ' + state.filters.date_range));
        chipsEl.innerHTML = frags.join('');
    }
    function chip(facet, value, label) {
        return '<span class="chip" data-facet="' + esc(facet) + '" data-value="' + esc(value) + '">' +
               esc(label) + '<button type="button" aria-label="Remove">×</button></span>';
    }

    // Update counts in place — don't rebuild the inputs, so focus/tab stays put.
    function renderFacets() {
        sidebar.querySelectorAll('label').forEach((lab) => {
            const box = lab.querySelector('input');
            const cnt = lab.querySelector('.facet-count');
            if (!box) return;
            const name = box.name, val = box.value;
            if (cnt) {
                const bucket = state.facets && state.facets[name];
                cnt.textContent = String((bucket && bucket[val]) || 0);
            }
            if (name === 'published') box.checked = state.filters.published === (val === 'true');
            else if (name === 'author') box.checked = state.filters.author.indexOf(val) !== -1;
            else if (name === 'date_range') box.checked = (state.filters.date_range || '') === val;
        });
    }

    function renderResults() {
        if (!state.hits || state.hits.length === 0) {
            const hasFilters = state.filters.published !== undefined ||
                               state.filters.author.length > 0 || !!state.filters.date_range;
            const head = state.q
                ? '<p>No results for <strong>' + esc(state.q) + '</strong>.</p>'
                : '<p>Start typing to search posts.</p>';
            const tail = hasFilters ? '<p><a href="#" id="search-clear-filters">Clear all filters</a></p>' : '';
            results.innerHTML = '<div class="search-empty">' + head + tail + '</div>';
            return;
        }
        results.innerHTML = state.hits.map(renderCard).join('');
    }

    function renderCard(hit) {
        const fm = hit._formatted || {};
        const title = fm.title || esc(hit.title || '');
        const body  = fm.body  || esc(hit.body  || '');
        const tag = hit.published
            ? '<span class="tag tag-published">Published</span>'
            : '<span class="tag tag-draft">Draft</span>';
        return '<article class="search-card" data-id="' + esc(hit.id) + '" tabindex="0">' +
               '<h2 class="search-card-title">' + title + '</h2>' +
               '<p class="search-card-body">' + body + '</p>' +
               '<div class="search-card-meta">' + tag +
                 '<span class="search-card-author">' + esc(hit.author || '') + '</span>' +
                 '<span class="search-card-dot">·</span>' +
                 '<span class="search-card-date" data-ts="' + esc(hit.created_at) + '">' + esc(hit.created_at_display || relativeTime(hit.created_at)) + '</span>' +
               '</div></article>';
    }

    function esc(s) {
        return String(s == null ? '' : s)
            .replace(/&/g, '&amp;').replace(/</g, '&lt;')
            .replace(/>/g, '&gt;').replace(/"/g, '&quot;');
    }

    // Mirrors search.rs::relative_time — keep them in lockstep.
    function relativeTime(ts) {
        const d = Math.max(0, Math.floor(Date.now()/1000) - Number(ts||0));
        if (d < 60) return 'just now';
        const u = d < 3600 ? [d/60,'minute'] : d < 86400 ? [d/3600,'hour']
              : d < 604800 ? [d/86400,'day'] : d < 2629800 ? [d/604800,'week']
              : d < 31557600 ? [d/2629800,'month'] : [d/31557600,'year'];
        const n = Math.floor(u[0]);
        return n + ' ' + u[1] + (n === 1 ? '' : 's') + ' ago';
    }

    // --- events -------------------------------------------------------------

    input.addEventListener('input', () => {
        state.q = input.value; state.page = 1;
        bar.classList.toggle('has-value', !!state.q);
        schedule();
    });
    clearBtn.addEventListener('click', () => {
        input.value = ''; state.q = '';
        bar.classList.remove('has-value');
        input.focus(); schedule();
    });
    sortSel.addEventListener('change', () => { state.sort = sortSel.value; state.page = 1; schedule(); });

    sidebar.addEventListener('change', (ev) => {
        const box = ev.target;
        if (!(box instanceof HTMLInputElement)) return;
        if (box.name === 'published' && box.type === 'checkbox') {
            const want = box.value === 'true';
            state.filters.published = box.checked ? want : undefined;
            // Mutually exclusive: checking one unchecks the other.
            if (box.checked) sidebar.querySelectorAll('input[name="published"]').forEach((o) => { if (o !== box) o.checked = false; });
        } else if (box.name === 'author' && box.type === 'checkbox') {
            const v = box.value;
            if (box.checked) { if (state.filters.author.indexOf(v) === -1) state.filters.author.push(v); }
            else             { state.filters.author = state.filters.author.filter((a) => a !== v); }
        } else if (box.name === 'date_range' && box.type === 'radio') {
            state.filters.date_range = box.value || undefined;
        }
        state.page = 1; schedule();
    });

    chipsEl.addEventListener('click', (ev) => {
        const btn = ev.target.closest('.chip button');
        if (!btn) return;
        const c = btn.parentElement;
        if      (c.dataset.facet === 'published')  state.filters.published = undefined;
        else if (c.dataset.facet === 'author')     state.filters.author = state.filters.author.filter((a) => a !== c.dataset.value);
        else if (c.dataset.facet === 'date_range') state.filters.date_range = undefined;
        state.page = 1; schedule();
    });

    results.addEventListener('click', (ev) => {
        if (ev.target.closest('#search-clear-filters')) {
            ev.preventDefault();
            state.filters = { author: [] }; state.page = 1; schedule();
            return;
        }
        const card = ev.target.closest('.search-card');
        if (card) open(card.dataset.id);
    });

    // --- keyboard -----------------------------------------------------------

    function updateSelection() {
        const cards = results.querySelectorAll('.search-card');
        cards.forEach((c, i) => c.classList.toggle('is-selected', i === selectedIndex));
        if (selectedIndex >= 0 && cards[selectedIndex]) cards[selectedIndex].scrollIntoView({ block: 'nearest' });
    }

    document.addEventListener('keydown', (ev) => {
        const typing = ev.target.matches('input, textarea, select, [contenteditable="true"]');
        if (ev.key === '/' && !typing) { ev.preventDefault(); input.focus(); input.select(); return; }
        if (ev.key === 'Escape' && ev.target === input) {
            input.value = ''; state.q = ''; bar.classList.remove('has-value'); schedule(); return;
        }
        if (ev.key === 'Enter' && ev.target === input) {
            ev.preventDefault();
            if (selectedIndex < 0) selectedIndex = 0;
            const cards = results.querySelectorAll('.search-card');
            if (cards[selectedIndex]) open(cards[selectedIndex].dataset.id);
            return;
        }
        if ((ev.key === 'ArrowDown' || ev.key === 'ArrowUp') && (ev.target === input || ev.target.closest('.search-card'))) {
            ev.preventDefault();
            const cards = results.querySelectorAll('.search-card');
            if (!cards.length) return;
            selectedIndex = ev.key === 'ArrowDown'
                ? Math.min(cards.length - 1, selectedIndex + 1)
                : Math.max(0, selectedIndex - 1);
            updateSelection();
        }
    });

    function open(id) { window.location.href = hrefTpl.replace('{id}', encodeURIComponent(id)); }

    bar.classList.toggle('has-value', !!state.q);
})();
