// Shared behaviour across every design-export page.
// Kept deliberately tiny — the pages are meant as a design surface,
// not a working app.

(function () {
  // ── Theme toggle (light / dark) ──────────────────────────────
  var stored = null;
  try { stored = localStorage.getItem('medflow-theme'); } catch (_) {}
  if (stored === 'dark' || stored === 'light') {
    document.documentElement.setAttribute('data-theme', stored);
  }

  function setTheme(next) {
    document.documentElement.setAttribute('data-theme', next);
    try { localStorage.setItem('medflow-theme', next); } catch (_) {}
    document.querySelectorAll('[data-theme-toggle] .sun').forEach(function (el) { el.hidden = next === 'dark'; });
    document.querySelectorAll('[data-theme-toggle] .moon').forEach(function (el) { el.hidden = next !== 'dark'; });
  }

  document.addEventListener('click', function (e) {
    var btn = e.target && e.target.closest ? e.target.closest('[data-theme-toggle]') : null;
    if (!btn) return;
    var current = document.documentElement.getAttribute('data-theme') || 'light';
    setTheme(current === 'dark' ? 'light' : 'dark');
  });

  // On load, sync the toggle icon state to whatever theme is in effect.
  document.addEventListener('DOMContentLoaded', function () {
    var current = document.documentElement.getAttribute('data-theme') || 'light';
    document.querySelectorAll('[data-theme-toggle] .sun').forEach(function (el) { el.hidden = current === 'dark'; });
    document.querySelectorAll('[data-theme-toggle] .moon').forEach(function (el) { el.hidden = current !== 'dark'; });
  });

  // ── Live character counters for textareas with [data-counter] ─
  document.addEventListener('input', function (e) {
    var el = e.target;
    if (!el.matches || !el.matches('[data-counter]')) return;
    var max = parseInt(el.getAttribute('maxlength') || '0', 10);
    var out = document.querySelector('[data-counter-for="' + el.id + '"]');
    if (!out) return;
    var n = el.value.length;
    out.textContent = n + ' / ' + max;
    out.classList.toggle('over', n > max);
  });

  // ── Duration preset / custom toggle (new-appointment form) ───
  var preset = document.getElementById('duration_preset');
  var custom = document.getElementById('duration_custom');
  if (preset && custom) {
    function syncDur() {
      if (preset.value === 'custom') { custom.hidden = false; }
      else { custom.hidden = true; }
    }
    preset.addEventListener('change', syncDur);
    syncDur();
  }
})();
