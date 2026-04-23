/* RustIO Admin — micro-interactions (v3, 2026-04-23).
 *
 * Small, carefully-scoped behaviors for a fully server-rendered
 * admin. No fetch calls, no client-side filtering, no routing.
 * Everything meaningful is a GET/POST form submission.
 *
 * Covers:
 *   - Bulk bar: show when rows are ticked, update count pill.
 *   - Select-all checkbox in thead.
 *   - Row click to open the edit drawer (ignoring clicks on
 *     checkboxes, action buttons, and form controls).
 *   - Keyboard: `/` focuses the toolbar search, `Esc` blurs an
 *     active input or closes the drawer (by navigating to `?`).
 *   - Field pill (`<label class="field-pill"><select>…</select>`):
 *     submit the parent toolbar form when the select changes so
 *     the filter takes effect without a separate submit button.
 *   - Switch (`.switch` wrapping `<input type="checkbox">`): keep
 *     the `.on` class in sync with checkbox state.
 */
(function () {
  'use strict';

  // ---------- Bulk bar visibility + count ----------
  function updateBulkBar() {
    var bar = document.querySelector('.bulk-bar');
    if (!bar) return;
    var checked = document.querySelectorAll('tbody .checkbox:checked');
    var count = checked.length;
    bar.classList.toggle('visible', count > 0);
    var pill = bar.querySelector('.bulk-bar-count');
    if (pill) pill.textContent = String(count);
  }

  document.addEventListener('change', function (e) {
    var t = e.target;
    if (!t) return;

    // Row checkbox
    if (t.classList && t.classList.contains('checkbox') && t.closest('tbody')) {
      var row = t.closest('tr');
      if (row) row.classList.toggle('selected', !!t.checked);
      updateBulkBar();
    }

    // Select-all
    if (t.id === 'check-all') {
      var on = !!t.checked;
      document.querySelectorAll('tbody .checkbox').forEach(function (cb) {
        if (cb.checked !== on) {
          cb.checked = on;
          var row = cb.closest('tr');
          if (row) row.classList.toggle('selected', on);
        }
      });
      updateBulkBar();
    }

    // Field pill select → submit parent form
    var pill = t.closest ? t.closest('.field-pill') : null;
    if (pill && t.tagName === 'SELECT') {
      var val = pill.querySelector('.field-pill-val');
      if (val) {
        var opt = t.options[t.selectedIndex];
        val.textContent = opt ? opt.textContent : '';
      }
      pill.classList.toggle('has-value', !!t.value);
      var form = t.closest('form');
      if (form) form.submit();
    }
  });

  // ---------- Row click to edit ----------
  document.addEventListener('click', function (e) {
    var t = e.target;
    if (!t) return;

    // Let checkboxes, buttons, forms, links, and labels handle
    // their own clicks — only bare td clicks should open the row.
    if (
      t.closest('.checkbox, button, a, form, label, input, select, textarea')
    ) {
      return;
    }
    var row = t.closest ? t.closest('tbody tr[data-edit-href]') : null;
    if (!row) return;
    var href = row.getAttribute('data-edit-href');
    if (href) window.location.assign(href);
  });

  // ---------- Switch toggle (label-wrapped checkbox) ----------
  document.addEventListener('click', function (e) {
    if (e.target && e.target.tagName === 'INPUT') return;
    var sw = e.target && e.target.closest ? e.target.closest('.switch') : null;
    if (!sw) return;
    var cb = sw.querySelector('input[type="checkbox"]');
    if (!cb) return;
    e.preventDefault();
    cb.checked = !cb.checked;
    sw.classList.toggle('on', cb.checked);
  });

  // ---------- Keyboard shortcuts ----------
  function isTyping() {
    var el = document.activeElement;
    if (!el || !el.tagName) return false;
    var t = el.tagName;
    return (
      t === 'INPUT' ||
      t === 'TEXTAREA' ||
      t === 'SELECT' ||
      el.isContentEditable === true
    );
  }
  document.addEventListener('keydown', function (e) {
    // "/" → focus the toolbar search
    if (e.key === '/' && !isTyping()) {
      var search = document.querySelector('.toolbar-search input');
      if (search) {
        e.preventDefault();
        search.focus();
        if (typeof search.select === 'function') search.select();
      }
      return;
    }
    // Cmd/Ctrl+K → also focus search
    if ((e.metaKey || e.ctrlKey) && (e.key === 'k' || e.key === 'K')) {
      var s2 = document.querySelector('.toolbar-search input');
      if (s2) {
        e.preventDefault();
        s2.focus();
      }
      return;
    }
    // Ctrl/Cmd+Enter inside a form → submit
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      var form =
        e.target && typeof e.target.closest === 'function'
          ? e.target.closest('[data-admin-form]')
          : null;
      if (form) {
        e.preventDefault();
        if (typeof form.requestSubmit === 'function') form.requestSubmit();
        else form.submit();
      }
      return;
    }
    // Esc → close drawer if open; else blur typing input
    if (e.key === 'Escape') {
      var drawer = document.querySelector('.drawer.open');
      if (drawer) {
        e.preventDefault();
        window.location.assign('?');
        return;
      }
      if (isTyping()) document.activeElement.blur();
    }
  });

  // ---------- Initial sync ----------
  updateBulkBar();
})();
