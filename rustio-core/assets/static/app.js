(function () {
  'use strict';

  // Confirm dialog for forms tagged with `data-confirm="…"`.
  document.addEventListener('submit', function (event) {
    var form = event.target;
    if (!(form instanceof HTMLFormElement)) return;
    var message = form.getAttribute('data-confirm');
    if (message && !window.confirm(message)) {
      event.preventDefault();
    }
  });

  // Theme toggle. The no-FOUC bootstrap in base.html has already set
  // <html data-theme>; this just keeps the toggle button's icon in sync
  // and persists the user's choice into localStorage.
  var btn = document.querySelector('.rio-theme-toggle');
  var icon = btn && btn.querySelector('[data-theme-icon]');
  function render() {
    var t = document.documentElement.getAttribute('data-theme');
    if (icon) icon.textContent = t === 'dark' ? '☼' : '☾';
  }
  render();
  if (btn) {
    btn.addEventListener('click', function () {
      var current = document.documentElement.getAttribute('data-theme');
      var next = current === 'dark' ? 'light' : 'dark';
      document.documentElement.setAttribute('data-theme', next);
      try { localStorage.setItem('rio-theme', next); } catch (_) {}
      render();
    });
  }

  // i18n L4b — language switcher. ONE handler for every `[data-lang-switcher]`
  // form (topbar + sidebar share the same component). On change, stamp the
  // current page into `_return` so the POST redirects back here, then submit.
  // With JS off the form still works (the <noscript> button submits; the
  // server falls back to /admin). Codes are submitted; endonyms are display.
  var switchers = document.querySelectorAll('[data-lang-switcher]');
  for (var i = 0; i < switchers.length; i++) {
    (function (form) {
      var ret = form.querySelector('[data-lang-return]');
      var sel = form.querySelector('select[name="lang"]');
      if (ret) ret.value = window.location.pathname + window.location.search;
      if (sel) {
        sel.addEventListener('change', function () {
          if (ret) ret.value = window.location.pathname + window.location.search;
          form.submit();
        });
      }
    })(switchers[i]);
  }
})();
