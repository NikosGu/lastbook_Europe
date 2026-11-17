'use strict';
/* The interface's words in six languages: English, Italian, German, French, Spanish and Polish. Loaded in <head>, before the
   page's own scripts, from the app's own files (/ui/locales/<lang>.json): no network.

   t(key, params)    the text for a key, as plain text: {name} placeholders take params.name; a number is written the way the
                     language writes numbers (1,234.5 or 1.234,5). A key whose value is an object has plural forms, chosen by
                     params.n with Intl.PluralRules ({one, few, many, other}, as each language needs).
   th(key, params)   the same, safe to put in HTML: the text and the params are escaped, except a param made with I18N.raw(html).
   I18N              lang, ready (a promise: the words are here), set(lang), apply(root) for data-i18n attributes, num, size,
                     unit, list, date, compare, country, raw, LANGS and NAME (each language's name in itself).

   Static HTML carries data-i18n="key" (its text) and data-i18n-attr="title=key;aria-label=key2" (attributes).
   The language is the person's choice, kept in localStorage 'lastbook.lang'. Until then: the first of navigator.languages
   that is one of the six, else English. A page re-renders itself on the 'lastbook:lang' event (setup.js, app.js). */
(() => {
  const LANGS = ['en', 'it', 'de', 'fr', 'es', 'pl'];
  const NAME = { en: 'English', it: 'Italiano', de: 'Deutsch', fr: 'Français', es: 'Español', pl: 'Polski' };
  const STORE = 'lastbook.lang';
  const VERSION = 2;   // part of the locale files' address: raise it with them, so no older copy is used
  const html = document.documentElement;

  function chosen() {
    try { const s = localStorage.getItem(STORE); if (LANGS.includes(s)) return s } catch { }
    for (const l of [].concat(navigator.languages || [], navigator.language || [])) {
      const p = String(l).toLowerCase().split(/[-_]/)[0];
      if (LANGS.includes(p)) return p;
    }
    return 'en';
  }

  let lang = chosen(), EN = {}, CUR = {}, F = null;
  const cache = {};   // locale files already read: switching back costs nothing
  const read = l => cache[l] || (cache[l] = fetch(`/ui/locales/${l}.json?v=${VERSION}`, { cache: 'no-store' })
    .then(r => r.ok ? r.json() : {}).catch(() => ({})).then(d => (d && typeof d === 'object' ? d : {})));

  // Formatters for the current language, made once per language
  function formatters(l) {
    const nf = new Intl.NumberFormat(l), pr = new Intl.PluralRules(l), units = {};
    let lf = null; try { lf = new Intl.ListFormat(l, { style: 'long', type: 'conjunction' }) } catch { }
    let col = null; try { col = new Intl.Collator(l, { sensitivity: 'base', numeric: true }) } catch { }
    let regions = null; try { regions = new Intl.DisplayNames([l], { type: 'region' }) } catch { }
    return { nf, pr, lf, col, regions, units };
  }

  const esc = s => String(s == null ? '' : s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const RAW = Symbol('raw');
  const raw = h => ({ [RAW]: String(h == null ? '' : h) });
  const value = (v, html) => v && typeof v === 'object' && RAW in v ? (html ? v[RAW] : v[RAW].replace(/<[^>]*>/g, ''))
    : typeof v === 'number' ? num(v) : html ? esc(v) : String(v == null ? '' : v);

  const EN_PLURAL = new Intl.PluralRules('en');
  function text(key, params) {
    const own = Object.hasOwn(CUR, key);
    let s = own ? CUR[key] : Object.hasOwn(EN, key) ? EN[key] : key;
    if (s && typeof s === 'object') {   // plural forms, by the rules of the language the text is in
      const n = params && typeof params.n === 'number' ? params.n : 0;
      s = s[(own ? F.pr : EN_PLURAL).select(n)] ?? s.other ?? '';
    }
    return String(s);
  }
  function fill(s, params, html) {
    return params ? s.replace(/\{(\w+)\}/g, (m, k) => Object.hasOwn(params, k) ? value(params[k], html) : m) : s;
  }
  const t = (key, params) => fill(text(key, params), params, false);
  const th = (key, params) => fill(esc(text(key, params)), params, true);

  /* ---- numbers, sizes, lists, dates, names */
  function num(n, opts) {
    if (n == null || !isFinite(n)) return '';
    return (opts ? new Intl.NumberFormat(lang, opts) : F.nf).format(n);
  }
  // A number with a unit the way the language writes it: 12,3 GB, 12,3 Go (French), 5 km, 300 m, 2 MB/s
  function unit(n, u, digits = 0) {
    const k = u + '|' + digits;
    if (!F.units[k]) {
      try { F.units[k] = new Intl.NumberFormat(lang, { style: 'unit', unit: u, unitDisplay: 'short', maximumFractionDigits: digits, minimumFractionDigits: digits }) }
      catch { const short = { gigabyte: 'GB', megabyte: 'MB', kilometer: 'km', meter: 'm', 'megabyte-per-second': 'MB/s' }[u] || u, plain = new Intl.NumberFormat(lang, { maximumFractionDigits: digits, minimumFractionDigits: digits }); F.units[k] = { format: x => plain.format(x) + ' ' + short } }
    }
    return F.units[k].format(n);
  }
  // Bytes as GB or MB. base: 1000, or 1024 on Windows, so the numbers match what Explorer shows (the unit stays GB)
  function size(b, base = 1000) {
    if (b == null || !isFinite(b)) return '';
    const g = b / base ** 3;
    if (g >= 100) return unit(Math.round(g), 'gigabyte');
    if (g >= 1) return unit(g, 'gigabyte', 1);
    const m = b / base ** 2;
    return m >= 10 ? unit(Math.round(m), 'megabyte') : unit(m, 'megabyte', 1);
  }
  function list(items, type = 'conjunction') {
    const a = [...items].map(String);
    if (type === 'conjunction' && F.lf) return F.lf.format(a);
    try { return new Intl.ListFormat(lang, { style: 'long', type }).format(a) } catch { return a.join(', ') }
  }
  /* A message the engine sends as {code, params} beside its English sentence: in the interface's language when the table
     (code → key) knows the code, else the engine's own English. A param named *_bytes is a size and *_mb_s a speed;
     opts.size formats sizes another way (setup counts in 1024s on Windows). */
  function said(table, msg, english, opts = {}) {
    const m = msg && typeof msg === 'object' ? msg : {};
    const key = typeof m.code === 'string' && Object.hasOwn(table, m.code) ? table[m.code] : null;
    if (!key) return english == null ? '' : String(english);
    const p = {};
    for (const [k, v] of Object.entries(m.params && typeof m.params === 'object' ? m.params : {}))
      p[k] = typeof v !== 'number' ? v : /_bytes$/.test(k) ? (opts.size || size)(v) : /_mb_s$/.test(k) ? unit(v, 'megabyte-per-second') : v;
    return t(key, p);
  }
  const date = (d, opts) => { try { return new Intl.DateTimeFormat(lang, opts).format(d instanceof Date ? d : new Date(d)) } catch { return String(d) } };
  const compare = (a, b) => F.col ? F.col.compare(String(a), String(b)) : String(a).localeCompare(String(b));
  const country = cc => { try { return (F.regions && F.regions.of(cc)) || cc } catch { return cc } };

  /* ---- the page's own HTML: data-i18n="key", data-i18n-attr="title=key;aria-label=key2" */
  function apply(root = document) {
    for (const el of root.querySelectorAll('[data-i18n]')) el.textContent = t(el.dataset.i18n);
    for (const el of root.querySelectorAll('[data-i18n-attr]')) {
      for (const pair of el.dataset.i18nAttr.split(';')) {
        const [a, k] = pair.split('=').map(s => s.trim());
        if (a && k) el.setAttribute(a, t(k));
      }
    }
  }

  /* ---- loading, and changing the language */
  html.lang = lang;
  html.classList.add('i18n-wait');   // the page shows once its words are here (a moment), so it never flashes English first
  const style = document.createElement('style');
  style.textContent = 'html.i18n-wait body{visibility:hidden}';
  document.head.append(style);
  F = formatters(lang);

  async function use(l) {
    const [en, cur] = await Promise.all([read('en'), l === 'en' ? null : read(l)]);
    EN = en; CUR = cur || en; lang = l; F = formatters(l);
    html.lang = l;
  }
  const shown = () => { apply(); html.classList.remove('i18n-wait') };
  const ready = use(lang).then(() => {
    if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', shown, { once: true }); else shown();
  });
  setTimeout(() => html.classList.remove('i18n-wait'), 3000);   // never a blank window, even if the files did not come

  async function set(l) {
    if (!LANGS.includes(l)) return;
    try { localStorage.setItem(STORE, l) } catch { }
    if (l === lang) return;
    await use(l);
    apply();
    dispatchEvent(new CustomEvent('lastbook:lang', { detail: l }));
  }

  // A language menu: a <select> listing each language in itself, and an element that shows the current one's name (the select
  // itself can be laid over it, invisible). The page decides where it goes and how it looks.
  function picker(sel, nameEl) {
    if (!sel) return;
    sel.innerHTML = LANGS.map(l => `<option value="${l}" lang="${l}"${l === lang ? ' selected' : ''}>${NAME[l]}</option>`).join('');
    const show = () => { sel.value = lang; if (nameEl) { nameEl.textContent = NAME[lang]; nameEl.lang = lang } };
    show();
    sel.onchange = () => set(sel.value);
    addEventListener('lastbook:lang', show);
  }

  window.I18N = Object.freeze({
    LANGS, NAME, get lang() { return lang }, ready, set, apply, picker,
    t, th, raw, esc, num, unit, size, list, date, compare, country, said,
  });
  window.t = t;
  window.th = th;
})();
