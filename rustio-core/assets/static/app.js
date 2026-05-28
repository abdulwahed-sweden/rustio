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
})();
