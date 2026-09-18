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

  // DW-1 — list filter controls. A `[data-filter]` select submits its toolbar
  // form on change; a text filter submits on Enter (native) or blur. With JS
  // off, the <noscript> Apply button covers it.
  var filterEls = document.querySelectorAll('[data-filter]');
  for (var f = 0; f < filterEls.length; f++) {
    var el = filterEls[f];
    var ev = el.tagName === 'SELECT' ? 'change' : 'change';
    el.addEventListener(ev, function (e) {
      var form = e.target.form;
      if (form) form.submit();
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
