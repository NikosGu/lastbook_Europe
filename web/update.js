/* Check for updates, in About (desktop app only). It runs only when the person presses Check for updates, or chooses
   Lastbook > Check for Updates… (the menu calls LBUpdate.fromMenu; no address starts it). It only says whether a newer version
   is on the website: nothing is downloaded or installed. crates/engine/src/update.rs does the check.
   Words: t() and th() from web/i18n.js, keys update.*. What the check says comes as a code with params (CODE), beside the
   engine's English sentence, which shows only for a code this page does not know. */
(() => {
  const E = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  let timer = null, menuAsked = false;
  // the engine's codes (crates/engine/src/update.rs CODES)
  const CODE = { unsupported: 'update.code.unsupported', no_site: 'update.code.no_site', network: 'update.code.network', certificate: 'update.code.certificate',
    refused: 'update.code.refused', offline: 'update.code.offline', not_published: 'update.code.not_published', server: 'update.code.server',
    bad_info: 'update.code.bad_info', too_old_os: 'update.code.too_old_os' };
  const said = (s, english) => E(I18N.said(CODE, s, english));

  async function call(action) {
    const r = await fetch('/api/update/' + action, action === 'state' ? {} : { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' });
    return r.json();
  }

  function view(s) {
    const btn = key => `<button class="btn plain" data-upd="check">${th(key)}</button>`;
    const ver = s.current ? th('update.version', { version: s.current }) : th('update.no_version');
    switch (s.state) {
      case 'checking': return `${ver} ${th('update.checking')}`;
      case 'up_to_date': return `${ver} ${th('update.up_to_date')} ${btn('update.check_again')}`;
      case 'available': return `${ver} ${th('update.available', { version: s.version })}${s.website ? ' ' + th('update.download_from', { site: s.website }) : ''} ${th('update.stays')} ${btn('update.check_again')}`;
      case 'not_published': return `${ver} ${said(s, s.message)} ${btn('update.try_again')}`;
      case 'no_site': return `${ver} ${said(s, s.message)}`;     // no download site chosen yet (crates/engine/src/dist.rs): nothing to try again
      case 'too_old_os': case 'error': return `${ver} ${said(s, s.message)} ${btn('update.try_again')}`;
      default: return `${ver} ${btn('update.check')}`;
    }
  }

  function paint(el, s) {
    el.innerHTML = view(s);
    clearTimeout(timer);
    if (s.state === 'checking') timer = setTimeout(() => refresh(el), 400);
  }

  async function refresh(el) {
    if (!el.isConnected) return;
    try { paint(el, await call('state')) } catch { el.textContent = t('update.failed') }
  }

  async function check(el) {
    try {
      const s = await call('check');
      if (s.error && !s.state) { el.textContent = I18N.said(CODE, s, s.error); return }
      paint(el, s);
    } catch { el.textContent = t('update.failed') }
  }

  window.LBUpdate = {
    card: () => `<div class="card"><b>${th('update.card_title')}</b><small id="updBox">…</small></div>`,
    mount(root) {
      const el = root.querySelector('#updBox'); if (!el) return;
      el.addEventListener('click', e => { if (e.target.closest('[data-upd]')) check(el) });
      if (menuAsked) { menuAsked = false; return check(el) }
      refresh(el);
    },
    fromMenu() {     // Lastbook > Check for Updates…: About, then one check
      const el = document.getElementById('updBox');
      if (el && location.hash.startsWith('#/about')) return check(el);
      menuAsked = true;
      location.hash = '#/about';
    },
  };
})();
