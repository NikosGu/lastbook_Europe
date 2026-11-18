'use strict';
/* Lastbook front end. Plain JS, no external resources. Talks only to the local server. */
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];
const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const fileUrl = p => '/' + String(p || '').split('/').map(encodeURIComponent).join('/');
const main = $('#main');
const IN_WINDOW = location.protocol === 'lastbook:' || location.hostname === 'lastbook.invalid';   // the desktop app, not a browser over the server
if (IN_WINDOW) $('#sideSettings').hidden = false;   // Library and local AI (web/setup.html) is in the desktop app only

/* ---------------------------------------------------------------- icons -- */
const I = {
  water: '<path d="M12 3s6 6.5 6 11a6 6 0 0 1-12 0c0-4.5 6-11 6-11z"/><path d="M9 14a3 3 0 0 0 3 3"/>',
  hurt: '<rect x="3" y="7" width="18" height="13" rx="2"/><path d="M9 7V5h6v2M12 10v7M8.5 13.5h7"/>',
  food: '<path d="M4 11h16a8 8 0 0 1-16 0zM8 7c0-1 1-1 1-2M12 7c0-1 1-1 1-2M16 7c0-1 1-1 1-2"/>',
  power: '<path d="M9 18h6M10 21h4M12 3a6 6 0 0 0-4 10.5c.7.7 1 1.5 1 2.5h6c0-1 .3-1.8 1-2.5A6 6 0 0 0 12 3z"/>',
  shelter: '<path d="M14 14.8V5a2 2 0 0 0-4 0v9.8a4 4 0 1 0 4 0zM12 9v8"/>',
  sanitation: '<path d="M7 3h10v6H7zM5 9h14l-2 6H7zM9 15l-1 6h8l-1-6"/>',
  disaster: '<path d="M12 3 2 20h20zM12 10v4M12 17h.01"/>',
  comms: '<path d="M5 12a7 7 0 0 1 14 0M8 12a4 4 0 0 1 8 0"/><circle cx="12" cy="12" r="1.5"/><path d="M12 13.5V21M9 21h6"/>',
  grow: '<path d="M12 21v-9M12 12c0-4 3-7 8-7 0 5-3 7-8 7zM12 14c0-3-2-5-7-5 0 4 2 5 7 5z"/>',
  preserve: '<path d="M8 3h8v3H8zM7 6h10l1 3v10a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V9z"/><path d="M6 12h12"/>',
  health: '<path d="M12 21s-8-4.5-8-11a4.5 4.5 0 0 1 8-2.8A4.5 4.5 0 0 1 20 10c0 6.5-8 11-8 11z"/><path d="M8 12h2l1-2 2 4 1-2h2"/>',
  energy: '<path d="M13 2 4 14h7l-1 8 9-12h-7z"/>',
  land: '<path d="M3 20 9 9l4 6 3-4 5 9z"/><circle cx="17" cy="6" r="2"/>',
  repair: '<path d="M14.7 6.3a4 4 0 0 0 5 5L21 13l-8 8-3-3 6.3-6.3a4 4 0 0 0-5-5L9 9 6 6l2-2"/><path d="M4 20l5-5"/>',
  map: '<path d="M9 4 3 6v14l6-2 6 2 6-2V4l-6 2zM9 4v14M15 6v14"/>',
  pin: '<path d="M12 21s-7-6.2-7-11.5a7 7 0 0 1 14 0C19 14.8 12 21 12 21z"/><circle cx="12" cy="9.5" r="2.5"/>',
  book: '<path d="M4 4h6a3 3 0 0 1 3 3v13a2 2 0 0 0-2-2H4zM20 4h-6a3 3 0 0 0-1 .2"/><path d="M20 4v14h-7"/>',
  video: '<rect x="3" y="5" width="18" height="14" rx="3"/><path d="m10 9 5 3-5 3z"/>',
  wiki: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/>',
  page: '<path d="M6 2h9l5 5v15H6z"/><path d="M14 2v6h6M9 13h7M9 17h7"/>',
  up: '<path d="M12 19V5M6 11l6-6 6 6"/>', back: '<path d="M15 18 9 12l6-6"/>', right: '<path d="m9 18 6-6-6-6"/>', down: '<path d="m6 9 6 6 6-6"/>',
  warn: '<path d="M12 3 2 20h20zM12 10v4M12 17h.01"/>', save: '<path d="M6 3h12v18l-6-4-6 4z"/>',
  ext: '<path d="M14 4h6v6M20 4l-9 9M18 14v6H4V6h6"/>', prep: '<path d="M9 11l3 3 8-8"/><path d="M20 12v7a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h9"/>',
  shelf: '<path d="M4 4v16M9 4v16M14 5l4 15M3 20h18"/>', search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-4-4"/>',
  play: '<path d="M7 4v16l13-8z"/>', clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  insp: '<rect x="3" y="5" width="18" height="14" rx="3"/><path d="M15 5v14"/>',
  ask: '<path d="M4 5h16v11H9l-5 4z"/><path d="M9 9.5h6M9 12.5h4"/>', spark: '<path d="M12 3v4M12 17v4M3 12h4M17 12h4M6 6l2.5 2.5M15.5 15.5 18 18M6 18l2.5-2.5M15.5 8.5 18 6"/>',
};
const ico = (n, cls = '') => `<svg viewBox="0 0 24 24" aria-hidden="true" class="${cls}">${I[n] || I.book}</svg>`;
const KIND_ICO = { pdf: 'page', html: 'page', video: 'video', transcript: 'video', pptx: 'page', zim: 'wiki' };
// One colour per guide, used for its tile, header and cards.
const TC = { hurt: '#c62828', water: '#1565c0', power: '#c77700', food: '#d4570f', shelter: '#7b3fa0', sanitation: '#7a5232', disaster: '#b23c17', comms: '#00796b',
  grow: '#2e7d32', preserve: '#a0661a', health: '#c2185b', repair: '#455a64', energy: '#e09400', land: '#5d7a2e' };
// Short names for tiles (keys in web/locales/); the full title shows on the guide page. chat.js uses them too: t(SHORT[id]).
const SHORT = { hurt: 'app.guide.hurt', water: 'app.guide.water', power: 'app.guide.power', food: 'app.guide.food', shelter: 'app.guide.shelter', sanitation: 'app.guide.sanitation',
  disaster: 'app.guide.disaster', comms: 'app.guide.comms', grow: 'app.guide.grow', preserve: 'app.guide.preserve', health: 'app.guide.health', repair: 'app.guide.repair', energy: 'app.guide.energy', land: 'app.guide.land' };
const shortOf = (id, fallback) => SHORT[id] ? t(SHORT[id]) : fallback;   // a guide's short name, else its own title
const tcStyle = id => `style="--tc:${TC[id] || '#1a6b55'}"`;
// Who a source is (crates/engine/src/library/catalog.rs TRUST), by its trust_code. Search's passages and the assistant's sources
// carry only the English label (trust): it is read through TRUST_EN to the same keys. A label neither knows shows as it came.
const TRUST = { humanitarian: 'app.trust.humanitarian', hesperian: 'app.trust.hesperian', government: 'app.trust.government', university: 'app.trust.university',
  course: 'app.trust.course', encyclopedia: 'app.trust.encyclopedia', hobbyist: 'app.trust.hobbyist', manufacturer: 'app.trust.manufacturer', reference: 'app.trust.reference' };
const TRUST_EN = { 'International health & humanitarian guidance': 'humanitarian', 'Hesperian community health guide': 'hesperian', 'Government guidance': 'government',
  'University / extension service': 'university', 'Course or textbook': 'course', 'Community-written encyclopedia': 'encyclopedia', 'Hobbyist demonstration video': 'hobbyist',
  'Manufacturer manual (model-specific)': 'manufacturer', 'Reference': 'reference' };
const trustOf = s => !s ? '' : I18N.said(TRUST, { code: Object.hasOwn(TRUST, s.trust_code) ? s.trust_code : Object.hasOwn(TRUST_EN, s.trust) ? TRUST_EN[s.trust] : null }, s.trust);
// What kind of item (catalog.rs KINDS), by its kind; a kind the table does not know, as the engine words it (kindLabel)
const KIND_NAME = { pdf: 'app.kind.pdf', html: 'app.kind.html', video: 'app.kind.video', transcript: 'app.kind.transcript', pptx: 'app.kind.pptx', zim: 'app.kind.zim', metadata: 'app.kind.metadata' };
const kindOf = d => I18N.said(KIND_NAME, { code: d.kind }, d.kindLabel || d.kind);
// Shelves and video rows by their ids, a row's note by its note_code (crates/engine/src/library/browse.rs), else as the engine words
// them. The channels' names stay names, in the table all the same, so a language could still change one.
const SHELF = { health: 'app.shelf.name.health', water: 'app.shelf.name.water', food: 'app.shelf.name.food', safety: 'app.shelf.name.safety', power: 'app.shelf.name.power',
  comms: 'app.shelf.name.comms', repair: 'app.shelf.name.repair', making: 'app.shelf.name.making', build: 'app.shelf.name.build', community: 'app.shelf.name.community',
  science: 'app.shelf.name.science', creators: 'app.shelf.name.creators', other: 'app.shelf.name.other' };
const VROW = { guides: 'app.vrow.name.guides', health: 'app.vrow.name.health', water: 'app.vrow.name.water', food: 'app.vrow.name.food', power: 'app.vrow.name.power',
  outdoors: 'app.vrow.name.outdoors', repair: 'app.vrow.name.repair', making: 'app.vrow.name.making', coalcracker: 'app.vrow.name.coalcracker', corporals: 'app.vrow.name.corporals',
  canterbury: 'app.vrow.name.canterbury', primitive: 'app.vrow.name.primitive', courses: 'app.vrow.name.courses', all: 'app.vrow.name.all' };
const VNOTE = { guides: 'app.vrow.note.guides', hobbyist: 'app.vrow.note.hobbyist', primitive: 'app.vrow.note.primitive', courses: 'app.vrow.note.courses' };
// A shelf's group of records without a collection, by its collection_code (browse.rs); a collection keeps its own name
const COLL = { other: 'app.coll.other' };
const shelfName = s => I18N.said(SHELF, { code: s.id }, s.title);
const rowName = r => I18N.said(VROW, { code: r.id }, r.title);
const rowNote = r => I18N.said(VNOTE, { code: r.note_code }, r.note);
const collName = c => I18N.said(COLL, { code: c.collection_code }, c.collection);

/* ------------------------------------------------------------ storage -- */
const store = {
  get(k, d) { try { return JSON.parse(localStorage.getItem('sg.' + k)) ?? d } catch { return d } },
  set(k, v) { try { localStorage.setItem('sg.' + k, JSON.stringify(v)) } catch { } },
};
// A row as the lists show it: its title and the subtitle under it. Two rows that read the same are one row, whatever their keys.
const rowSub = x => x.sub && x.sub !== x.title ? x.sub : x.key[0] === 'w' ? t('app.row.encyclopedia') : '';
const rowId = x => x.title + '|' + rowSub(x);
function remember(item) {  // {key,title,href,sub}
  const r = store.get('recent', []).filter(x => x.key !== item.key && rowId(x) !== rowId(item));
  r.unshift({ ...item, at: Date.now() });
  store.set('recent', r.slice(0, 40));
}
const isSaved = key => store.get('saved', []).some(x => x.key === key);
function savedCount() { const el = $('#savedN'); if (el) el.textContent = store.get('saved', []).length || '' }
function toggleSave(item, btn) {
  let s = store.get('saved', []);
  if (s.some(x => x.key === item.key)) { s = s.filter(x => x.key !== item.key); toast(t('app.save.removed')) }
  else { s.unshift({ ...item, at: Date.now() }); toast(t('app.save.saved')) }
  store.set('saved', s);
  savedCount();
  if (btn) { btn.setAttribute('aria-pressed', isSaved(item.key)); const sp = $('span', btn); if (sp) sp.textContent = isSaved(item.key) ? t('app.save.done') : t('app.save.save') }
}
const saveButton = item => `<button class="btn plain" title="${th('app.save.save')}" data-save='${esc(JSON.stringify(item))}' aria-pressed="${isSaved(item.key)}">${ico('save')}<span>${isSaved(item.key) ? th('app.save.done') : th('app.save.save')}</span></button>`;
document.addEventListener('click', e => { const b = e.target.closest('[data-save]'); if (b) toggleSave(JSON.parse(b.dataset.save), b) });
let toastT;
function toast(msg) {
  let box = $('.toast'); if (!box) { box = document.createElement('div'); box.className = 'toast'; box.setAttribute('role', 'status'); document.body.append(box) }
  box.textContent = msg; box.hidden = false; clearTimeout(toastT); toastT = setTimeout(() => box.hidden = true, 2200);
}
// Video watch progress: {id: {t, d, title, thumb, at}}
const vprog = () => store.get('vprog', {});
// This device's history is kept for every library opened on it (each download is its own library): show only what the open one holds
const HELD = {};
const held = ids => Promise.all(ids.map(id => HELD[id] ?? (HELD[id] = fetch('/api/doc/' + encodeURIComponent(id)).then(r => r.status !== 404 && r.status !== 400).catch(() => true)))).then(ok => ids.filter((_, i) => ok[i]));
async function resumable(n = 60) {
  const list = Object.entries(vprog()).filter(([, v]) => v.t > 10 && (!v.d || v.t < v.d - 15)).sort((a, b) => b[1].at - a[1].at).slice(0, n);
  const ok = new Set(await held(list.map(([id]) => id)));
  return list.filter(([id]) => ok.has(id));
}
async function recents() {   // documents and videos from another library leave the list; guides and articles stay
  const r = store.get('recent', []), ids = r.filter(x => /^[dv]:/.test(x.key)).map(x => x.key.slice(2));
  const ok = new Set(await held(ids)), seen = new Set();
  return r.filter(x => (!/^[dv]:/.test(x.key) || ok.has(x.key.slice(2))) && !seen.has(rowId(x)) && seen.add(rowId(x)));
}
function setVprog(id, data) { const p = vprog(); p[id] = { ...(p[id] || {}), ...data, at: Date.now() }; const keys = Object.keys(p).sort((a, b) => p[b].at - p[a].at).slice(0, 60); store.set('vprog', Object.fromEntries(keys.map(k => [k, p[k]]))) }

/* --------------------------------------------------------------- data -- */
async function api(u, opts) {
  const r = await fetch(u, opts);
  let d; try { d = await r.json() } catch { throw new Error(t('app.err.unreadable')) }
  if (away.said(r, d)) throw Object.assign(new Error(d.error || d.code), { away: true });   // the banner says it: no page shows this error
  if (!r.ok || (d && d.error && !Array.isArray(d))) throw new Error(d && d.error || t('app.err.server', { status: String(r.status) }));
  return d;
}
/* The library out of reach: its disk unplugged, or its network share gone. The engine then answers every library request with
   503 {code: 'library_unreachable', folder} (crates/engine/src/router.rs), and /api/health with {library: false}. One banner at the
   top of the window says so, whichever request heard it (fetch is watched for that answer), and /api/health is asked every 5 s
   while it shows. Once the library answers again the banner goes and the page is drawn again. Meanwhile no page shows an error of
   its own: api() throws an error marked away, which the error boxes leave out, and here() tells a page whose file failed to load. */
const away = (() => {
  const fetch0 = window.fetch.bind(window);
  let folder = null, timer = 0, box = null;
  const paint = () => {
    if (folder === null) return;
    if (!box) { box = document.createElement('div'); box.className = 'away'; box.setAttribute('role', 'alert'); $('header.top').after(box) }
    box.innerHTML = `<div class="away-in">${ico('warn')}<span>${th('app.unreachable.text', { folder: I18N.raw(`<b>${esc(folder)}</b>`) })}</span></div>`;
    box.hidden = false;
  };
  const show = f => { const was = folder !== null; folder = String(f ?? ''); paint(); if (!was) timer = setTimeout(check, 5000) };
  async function check() {
    let back = false;
    try { back = (await (await fetch0('/api/health', { cache: 'no-store' })).json()).library === true } catch { }
    if (!back) { timer = setTimeout(check, 5000); return }
    folder = null; clearTimeout(timer); if (box) { box.hidden = true; box.replaceChildren() }
    route();
  }
  // whether a response is the library out of reach (and then the banner shows); d: its JSON, when already read
  const said = (r, d) => r.status === 503 && !!d && d.code === 'library_unreachable' && (show(d.folder), true);
  window.fetch = async (...a) => {
    const r = await fetch0(...a);
    if (r.status === 503) r.clone().json().then(d => said(r, d)).catch(() => { });
    return r;
  };
  // false when the library does not answer: a page's file that failed to load (a picture, a video) is then the banner's to tell
  const here = async () => { try { const r = await fetch0('/api/library'); return !(r.status === 503 && said(r, await r.json())) } catch { return true } };
  addEventListener('lastbook:lang', paint);
  return { said, here, get on() { return folder !== null } };
})();
let LIB = null;
const libInfo = () => LIB || (LIB = api('/api/library').catch(() => { LIB = null; return {} }));   // what this library holds; a failed ask is asked again. An older server leaves out zims, pins and sources: then everything stays offered.
let TOPICS = null, TOPICS_LANG = '';   // the guides, in the language they were asked in: another language asks again
async function topics() {
  if (TOPICS && TOPICS_LANG === I18N.lang) return TOPICS;
  try { TOPICS = await api('/api/topics?lang=' + I18N.lang); TOPICS_LANG = I18N.lang; store.set('topicsCache', TOPICS) }
  catch (e) { TOPICS = TOPICS || store.get('topicsCache', null); if (!TOPICS) throw e }
  return TOPICS;
}
const topicById = id => (TOPICS || []).find(t => t.id === id);
const NOW_ORDER = ['hurt', 'water', 'power', 'food', 'shelter', 'sanitation', 'disaster', 'comms'];
const LEARN_ORDER = ['grow', 'preserve', 'health', 'repair', 'energy', 'land'];
const nowTopics = () => NOW_ORDER.map(topicById).filter(Boolean).concat((TOPICS || []).filter(t => t.mode === 'now' && !NOW_ORDER.includes(t.id)));
const learnTopics = () => LEARN_ORDER.map(topicById).filter(Boolean).concat((TOPICS || []).filter(t => t.mode === 'learn' && !LEARN_ORDER.includes(t.id)));

/* ------------------------------------------------------------ helpers -- */
const fmtDur = s => { if (!s) return ''; s = Math.round(s); const h = s / 3600 | 0, m = (s % 3600) / 60 | 0; return h ? t('app.dur.hm', { h, m }) : m ? t('app.dur.m', { m }) : t('app.dur.s', { s }) };
const clock = s => { s = Math.max(0, Math.round(s || 0)); const h = s / 3600 | 0, m = (s % 3600) / 60 | 0, x = s % 60; return (h ? h + ':' + String(m).padStart(2, '0') : m) + ':' + String(x).padStart(2, '0') };
const here = () => location.hash || '#/videos';
const fromQ = () => 'from=' + encodeURIComponent(here());
const pageImg = (id, p, w) => `/api/page/${encodeURIComponent(id)}/${p || 1}.png?w=${w}`;
const hostOf = u => { try { return new URL(u).host } catch { return u } };   // a web address shown as its site only
const askLink = (about, label = t('app.ask.about')) => `<a class="btn plain" title="${esc(label)}" href="#/ask?about=${encodeURIComponent(about)}&${fromQ()}">${ico('ask')}<span>${esc(label)}</span></a>`;

function hrefFor(item, src) {
  src = src || item.src || item;
  if (item.zim) return '#/wiki/' + item.zim.replace(/^\/?zim\//, '') + '?' + fromQ();
  if (!src || !src.id) return '#/';
  if (src.kind === 'video') return `#/watch/${encodeURIComponent(src.id)}?${item.t ? 't=' + item.t + '&' : ''}${fromQ()}`;
  return `#/read/${encodeURIComponent(src.id)}?${item.page ? 'p=' + item.page + '&' : ''}${fromQ()}`;
}
function srcText(s, page) {
  if (!s) return th('app.src.not_found');
  const bits = [`<strong>${esc(s.publisher || s.collection || t('app.src.unknown_publisher'))}</strong>`];
  if (s.year) bits.push(esc(s.year));
  if (page) bits.push(s.pages ? th('app.src.page_of', { page, pages: s.pages }) : th('app.src.page', { page }));
  else if (s.kind === 'pdf' && s.pages) bits.push(th('app.src.pages', { n: s.pages }));
  if (s.duration) bits.push(esc(fmtDur(s.duration)) + (s.transcript ? ' · ' + th('app.src.transcript') : ''));
  return bits.join(' · ');
}
// A reading card shows a picture of what you'll open: the actual page, the video frame, or an encyclopedia mark.
function rcard(item) {
  const s = item.src;
  let pv;
  if (item.zim) pv = `<span class="pv w">${ico('wiki')}</span>`;
  else if (s && s.kind === 'video') pv = `<span class="pv v">${s.thumb ? `<img loading="lazy" alt="" src="${fileUrl(s.thumb)}">` : ''}<span class="play"><span>${ico('play')}</span></span>${s.duration ? `<span class="pg">${clock(s.duration)}</span>` : ''}</span>`;
  else if (s && s.kind === 'pdf') pv = `<span class="pv"><img loading="lazy" alt="" src="${pageImg(s.id, item.page, 360)}" onerror="this.remove()"><span class="pg">${item.page ? th('app.src.p', { page: item.page }) : 'PDF'}</span></span>`;
  else pv = `<span class="pv w">${ico('page')}</span>`;
  const label = item.label || item.title || (s && s.title);
  const src = item.zim ? `<strong>${th('app.row.encyclopedia')}</strong> · ${esc(item.title || '')} · ${th('app.src.community')}` : `${s ? th('app.src.from', { title: s.title }) + ' · ' : ''}${srcText(s, item.page)}<br>${esc(trustOf(s))}`;
  return `<a class="rcard" href="${hrefFor(item)}">${pv}<span><b>${esc(label)}</b>${item.note ? `<p>${esc(item.note)}</p>` : ''}<span class="src">${src}</span></span></a>`;
}
function rowItem(item) {
  const s = item.src || null;
  let k = ico(item.zim ? 'wiki' : s ? KIND_ICO[s.kind] : 'page');
  if (s && s.kind === 'video' && s.thumb) k = `<img loading="lazy" alt="" src="${fileUrl(s.thumb)}">`;
  else if (s && s.kind === 'pdf' && (item.page || item.preview)) k = `<img loading="lazy" alt="" src="${pageImg(s.id, item.page, 200)}" onerror="this.remove()">`;
  const sub = item.zim ? th('app.row.article_community') : srcText(s, item.page);
  return `<li><a class="row" href="${hrefFor(item)}"><span class="k">${k}</span><span><b>${esc(item.label || item.title || (s && s.title))}</b><small>${sub}</small></span></a></li>`;
}
function vcard(v, opts = {}) {
  const p = vprog()[v.id];
  const pct = p && p.t && (v.duration || p.d) ? Math.min(100, 100 * p.t / (v.duration || p.d)) : 0;
  return `<a class="vcard" href="#/watch/${encodeURIComponent(v.id)}?${fromQ()}" ${opts.style || ''}>
    <span class="vthumb">${v.thumb ? `<img loading="lazy" alt="" src="${fileUrl(v.thumb)}" onerror="this.remove()">` : `<span class="ph">${ico('video')}</span>`}
      ${v.transcript ? `<span class="cc" title="${th('app.video.has_transcript')}">CC</span>` : ''}${v.duration ? `<span class="dur">${clock(v.duration)}</span>` : ''}${pct > 2 ? `<span class="prog" style="width:${pct}%"></span>` : ''}<span class="play">${ico('play')}</span></span>
    <b>${esc(v.title)}</b><span class="vm">${esc(opts.sub || v.publisher || '')}</span></a>`;
}
function rail(html, cls = '') {
  return `<div class="rail-wrap"><button class="rail-btn l" aria-label="${th('app.rail.left')}" hidden>${ico('back')}</button><div class="rail ${cls}">${html}</div><button class="rail-btn r" aria-label="${th('app.rail.right')}">${ico('right')}</button></div>`;
}
function wireRails(root = main) {
  $$('.rail-wrap', root).forEach(w => {
    const r = $('.rail', w), l = $('.rail-btn.l', w), rb = $('.rail-btn.r', w);
    const upd = () => { l.hidden = r.scrollLeft < 10; rb.hidden = r.scrollLeft + r.clientWidth > r.scrollWidth - 10 };
    l.onclick = () => r.scrollBy({ left: -r.clientWidth * .85, behavior: 'smooth' });
    rb.onclick = () => r.scrollBy({ left: r.clientWidth * .85, behavior: 'smooth' });
    r.addEventListener('scroll', upd, { passive: true }); upd();
  });
}
const secHead = (title, sub, link) => `<div class="sec-head"><div><h2>${title}</h2>${sub ? `<span class="sub">${sub}</span>` : ''}</div>${link || ''}</div>`;
function crumbs(list) {
  return `<nav class="crumbs" aria-label="${th('app.crumbs')}">${list.map((c, i) => i < list.length - 1 ? `<a href="${c[1]}">${esc(c[0])}</a><span aria-hidden="true">›</span>` : `<span aria-current="page">${esc(c[0])}</span>`).join('')}</nav>`;
}
function backButton(from, fallback = t('app.back.back')) {
  let label = fallback, href = from || '#/';
  const m = /^#\/t\/([\w-]+)/.exec(href);
  if (m && topicById(m[1])) label = shortOf(m[1], topicById(m[1]).title);
  else if (href.startsWith('#/search')) label = t('app.back.search');
  else if (href.startsWith('#/ask')) label = t('app.back.chat');
  else if (href.startsWith('#/saved')) label = t('app.saved.title');
  else if (href.startsWith('#/map')) label = t('nav.map');
  else if (href.startsWith('#/videos')) label = t('nav.videos');
  else if (href.startsWith('#/watch')) label = t('app.back.video');
  else if (href.startsWith('#/read')) label = t('app.back.page');
  else if (href.startsWith('#/library') || href.startsWith('#/shelf') || href.startsWith('#/collection') || href.startsWith('#/encyclopedias') || href.startsWith('#/prepare')) label = t('app.lib.title');
  else if (href === '#/' || href === '#') label = t('nav.home');
  return `<a class="back" href="${esc(href)}" data-back>${ico('back')}<span>${esc(label)}</span></a>`;
}
// Links into readers carry where you came from; refresh it at click time (sections may have opened since render).
document.addEventListener('click', e => {
  const a = e.target.closest('a[href^="#/read/"],a[href^="#/watch/"],a[href^="#/wiki/"],a[href^="#/collection"],a[href^="#/ask?about"]');
  if (!a || a.hasAttribute('data-back')) return;
  const h = a.getAttribute('href'), i = h.indexOf('from=');
  if (i > -1) a.setAttribute('href', h.slice(0, i) + 'from=' + encodeURIComponent(here()));
}, true);
// Prefer real history so scroll position and open sections come back exactly (not inside encyclopedia frames).
document.addEventListener('click', e => {
  const a = e.target.closest('a[data-back]');
  if (a && !location.hash.startsWith('#/wiki/') && navAt > 0 && navStack[navAt - 1] === a.getAttribute('href')) { e.preventDefault(); history.back() }
});
function errorBox(e, what) {
  if (e && e.away) return '';   // the library out of reach: the banner says so (away)
  return `<div class="err" role="alert"><b>${esc(what || t('app.err.generic'))}</b><br>${esc(e.message || e)}
  <br><span class="small">${th('app.err.persist')}</span></div>`;
}
// The window title, and on reading pages the toolbar's title and the line under it.
// bar: what the toolbar shows when it is not the page's own title (an article names itself; the toolbar names the encyclopedia).
const setTitle = (t, sub, bar) => { document.title = t ? t + ' · Lastbook Europe' : 'Lastbook Europe'; const b = $('#tbTitle'); b.textContent = b.title = bar ?? t ?? ''; if (sub !== undefined) $('#tbSub').textContent = $('#tbSub').title = sub };

/* ------------------------------------------------------------- router -- */
// Where this tab is in its history. Each entry carries its own index in history.state, so Back and Forward are known exactly:
// after going back several steps, after a page rewrote its address in place, and when a link leads to the page just left.
const navStack = JSON.parse(sessionStorage.getItem('sg.stack') || '[]');   // navStack[i]: the address at history index i
let navAt = +(sessionStorage.getItem('sg.at') || 0), navEnd = +(sessionStorage.getItem('sg.end') || 0), swapping = false;
const keepNav = () => { try { sessionStorage.setItem('sg.stack', JSON.stringify(navStack.slice(0, navEnd + 1))); sessionStorage.setItem('sg.at', navAt); sessionStorage.setItem('sg.end', navEnd) } catch { } };
const _replace = history.replaceState.bind(history);
// A page that rewrites its own address (the map, search as you type, an opened situation, a new chat) keeps its place.
history.replaceState = (state, title, url) => { _replace({ ...(state || {}), lb: navAt }, title, url); if (url != null) { navStack[navAt] = current = here(); keepNav() } };
const swap = h => { swapping = true; location.replace(h) };   // this entry becomes another page, in place
const _push = history.pushState.bind(history);
const pushHere = url => { navAt++; navEnd = navAt; navStack.length = navAt; _push({ lb: navAt }, '', url); navStack[navAt] = current = here(); keepNav(); barState() };   // a new entry, without loading the page again
const scrolls = JSON.parse(sessionStorage.getItem('sg.scroll') || '{}');
try { history.scrollRestoration = 'manual' } catch { }   // the page restores its own place (restoreScroll), once it is built
let current = null;
window.addEventListener('scroll', () => { if (current && !$('#wframe')) scrolls[current] = scrollY }, { passive: true });   // an article scrolls in its frame, which keeps its own place (wiki)
window.addEventListener('pagehide', () => sessionStorage.setItem('sg.scroll', JSON.stringify(scrolls)));
function parse() {
  const h = here().slice(1) || '/';
  const [path, qs] = h.split('?');
  return { parts: path.split('/').filter(Boolean).map(decodeURIComponent), q: new URLSearchParams(qs || '') };
}
// The sidebar row each page belongs to. Reading pages stay under Library, articles under Encyclopedias; search results select no row.
const NAV_OF = { '': 'home', t: 'home', ask: 'ask', videos: 'videos', watch: 'videos', map: 'map', library: 'library', learn: 'library', browse: 'library', shelf: 'library', collection: 'library', read: 'library', wiki: 'encyclopedias', search: '', saved: 'saved', encyclopedias: 'encyclopedias', prepare: 'prepare', about: 'about' };
let leaving = null;  // cleanup hook for the current view (player, chat stream…)
async function route() {
  const cameFromSide = !!(document.activeElement && document.activeElement.closest && document.activeElement.closest('.side,.tabbar'));
  const { parts, q } = parse();
  const h = here();
  const wf = $('#wframe'), wd = wf && wf.contentDocument;
  if (current) { if (!wf) scrolls[current] = scrollY; else if (wd && wd.readyState === 'complete') scrolls[current] = wf.contentWindow.scrollY }   // an article's place is its frame's
  const at = history.state && history.state.lb;
  if (typeof at === 'number') { navAt = at; navEnd = Math.max(navEnd, at) }   // back, forward, or a reload
  else {
    if (swapping) swapping = false;
    else if (current !== null || navStack.length) { navAt++; navEnd = navAt; navStack.length = navAt }   // a new page: what was ahead is gone
    else navAt = navEnd = 0;   // the first page of this tab
    _replace({ ...(history.state || {}), lb: navAt }, '');
  }
  navStack[navAt] = h; keepNav();
  sessionStorage.setItem('sg.scroll', JSON.stringify(scrolls));
  current = h;
  const y = typeof at === 'number' ? scrolls[h] : null;   // read now: while the page is rebuilt it scrolls (shorter, then taller), and those scrolls are recorded too
  const v = parts[0] || '';   // the home page: the guides and the search (the starter library is reading, not videos)
  document.body.className = v ? 'v-' + v : 'home';
  const nav = v === 'read' && (q.get('from') || '').startsWith('#/about') ? 'about' : NAV_OF[v] || '';   // Sources, opened from About, stays under About
  $$('[data-nav]').forEach(a => nav && (a.dataset.nav === nav || (a.dataset.also || '').split(' ').includes(nav)) ? a.setAttribute('aria-current', 'page') : a.removeAttribute('aria-current'));
  const rows = $$('.side a.row-s'); rows.forEach(a => a.tabIndex = a.hasAttribute('aria-current') || (!nav && a === rows[0]) ? 0 : -1);   // the sidebar is one Tab stop; arrows move inside it
  savedCount(); railTitles();
  html.classList.remove('side-open', 'h1-gone'); h1seen.disconnect();
  $('#tbActs').replaceChildren(); $('#tbTitle').textContent = $('#tbSub').textContent = ''; $('#tbTitle').title = $('#tbSub').title = ''; searchLive = null; main.classList.remove('own-title');
  if (leaving) { try { leaving() } catch { } leaving = null }
  window.onresize = null; main.onkeydown = null;
  try {
    if (v !== 'map') MapView.stop();
    await topics().catch(() => { });
    if (!v) await home();
    else if (v === 't') await topicPage(parts[1], parts[2]);
    else if (v === 'ask') await Chat.page(main, parts.slice(1), q);
    else if (v === 'videos') await (parts[1] ? videoRow(parts[1], q) : videosHome());
    else if (v === 'watch') await watch(parts[1], +q.get('t') || 0, q.get('from'));
    else if (v === 'read') await reader(parts[1], +q.get('p') || 0, q.get('from'), q.get('view'));
    else if (v === 'wiki') await wiki(parts.slice(1).join('/'), q.get('from'), y);
    else if (v === 'search') await searchPage(q.get('q') || '', q.get('kind') || 'all');
    else if (v === 'library' || v === 'learn' || v === 'browse') await library();
    else if (v === 'prepare') await prepareHub();
    else if (v === 'encyclopedias') await encyclopedias(q.get('q') || '');
    else if (v === 'map') await MapView.page(main, q);
    else if (v === 'saved') await savedPage();
    else if (v === 'shelf') await shelfPage(parts[1], q.get('kind') || 'all', q.get('q') || '');
    else if (v === 'collection') await collectionPage(q.get('name') || '', q.get('from'));
    else if (v === 'about') await about();
    else notFound();
  } catch (e) { setTitle(''); document.body.className = 'v-error'; main.innerHTML = e && e.away ? '' : emptyState('warn', t('app.err.page'), e.message || e) }   // v-error: not a reading page, so a phone keeps its tab bar. The library out of reach: the banner says so (away)
  wireRails(); dock(); barState();
  $('#topQ').value = '';
  const big = $('main h1'); if (big) h1seen.observe(big);
  if (v === 'ask') { bandState(); return }  // the chat manages its own scroll position
  if (y != null && v !== 'wiki') restoreScroll(h, y); else if (v !== 'wiki') requestAnimationFrame(() => { if (!$('.sit.open', main)) scrollTo(0, 0) });
  if (y == null) startHere(cameFromSide);
  bandState(); alignTitle();
}
// Back, Forward or a reload return to the place the page was left. A page still filling in (pictures, results) may not be tall enough yet,
// so this tries again each frame for a moment (at most 2.5 s), and stops as soon as the person scrolls or the page changes.
function restoreScroll(h, y, w = window) {
  const stop = new AbortController(), end = performance.now() + 2500, o = { passive: true, signal: stop.signal };
  for (const ev of ['wheel', 'touchstart', 'keydown', 'mousedown']) addEventListener(ev, () => stop.abort(), o);
  let sh = 0, still = 0, seen = null;   // done once the place is reached and the page has stopped growing for a moment (sections still arriving would move it)
  const tick = () => {
    if (stop.signal.aborted || (w === window ? current !== h : !(w.frameElement && w.frameElement.isConnected))) return;   // another page, or another article in the frame
    const h2 = w.document.scrollingElement.scrollHeight;
    if (seen !== null && w.scrollY !== seen && h2 === sh) return stop.abort();   // it moved while the page did not grow: the person scrolled (an article's own wheel and key events never arrive in the Mac app)
    if (Math.abs(w.scrollY - y) > 1) w.scrollTo(0, y);
    seen = w.scrollY; still = h2 === sh && Math.abs(seen - y) <= 1 ? still + 1 : 0; sh = h2;
    if (still < 12 && performance.now() < end) requestAnimationFrame(tick); else stop.abort();
  };
  requestAnimationFrame(tick);
}
// On Windows the toolbar's title starts where the page's column does, as in Fluent's reading apps. On the Mac it follows Back and Forward.
function alignTitle() {
  const t = $('.tb-title'); if (!t || MAC) return;
  t.style.marginLeft = '';
  if (!t.offsetWidth) return;
  const d = main.getBoundingClientRect().left + parseFloat(getComputedStyle(main).paddingLeft) - t.getBoundingClientRect().left;
  if (d > 0 && d < 240) t.style.marginLeft = parseFloat(getComputedStyle(t).marginLeft) + d + 'px';
}
addEventListener('resize', alignTitle);
// After a new page: a person moving through the sidebar stays there, as in Finder or Settings, and Tab goes on to the toolbar and the page.
// Otherwise the next Tab starts at the top of the window (skip link, sidebar, toolbar, page), as after a full load. The marker it starts from
// is not a control; it carries the page's name, which a screen reader reads when focus lands on it.
function startHere(fromSide) {
  if (fromSide && !(MAC && !wide.matches) && document.activeElement && document.activeElement.closest('.side,.tabbar')) return;
  const t = $('#pageTop'), h1 = $('h1', main);
  t.textContent = (h1 && h1.textContent.trim()) || document.title;
  t.focus({ preventScroll: true });
}
window.addEventListener('hashchange', route);
let searchLive = null;   // on the search page, where the pane's field is the page's own (Windows), typing there updates the results
$('#topSearch').addEventListener('submit', e => { e.preventDefault(); if (searchLive) { searchLive($('#topQ').value.trim(), true); $('#topQ').blur(); return } location.hash = '#/search?q=' + encodeURIComponent($('#topQ').value.trim()) });
$('#topQ').addEventListener('input', () => { if (searchLive) searchLive($('#topQ').value.trim(), false, true) });

/* --------------------------------------------------------- the window -- */
const html = document.documentElement, MAC = html.classList.contains('plat-mac');
const wide = matchMedia('(min-width:900px)');
// The toolbar names the page once its large title has scrolled up under the toolbar.
const h1seen = new IntersectionObserver(es => html.classList.toggle('h1-gone', !es[es.length - 1].isIntersecting), { rootMargin: `-${(parseInt(getComputedStyle(html).getPropertyValue('--top-h')) || 52) + (parseInt(getComputedStyle(html).getPropertyValue('--cap-h')) || 0)}px 0px 0px 0px` });
// Windows' folded pane shows icons only: each row then carries its name as a tooltip, which is also its name for a screen reader.
function railTitles() { const icons = html.classList.contains('compact') && !html.classList.contains('side-open'); $$('.side a.row-s').forEach(a => icons ? a.title = $('span', a).textContent : a.removeAttribute('title')) }
addEventListener('resize', railTitles);
// The toolbar's field says only 'Search' when it is too narrow for the whole hint, as Windows' and the Mac's own narrow fields do
const topPlaceholder = () => { const q = $('#topQ'); if (q.clientWidth) q.placeholder = q.clientWidth < 175 ? t('app.search.short') : t('nav.search_library') };
I18N.ready.then(() => new ResizeObserver(topPlaceholder).observe($('#topQ')));   // the words come first
// At 900 and wider a page's .dock controls (view switch, Save, Ask) sit in the toolbar; narrower they stay in the page.
const docks = () => wide.matches || (!MAC && innerWidth >= 640);
function dock() {
  if (docks()) $$('.dock', main).forEach(el => { const mark = document.createElement('i'); mark.className = 'dock-home'; mark.hidden = true; el.before(mark); $('#tbActs').append(el) });
  else $$('#tbActs .dock').forEach(el => { const mark = $('.dock-home', main); if (mark) mark.replaceWith(el); else el.remove() });
}
wide.addEventListener('change', () => { html.classList.remove('side-open'); dock(); railTitles(); bandState() });
matchMedia('(min-width:640px)').addEventListener('change', () => { dock(); bandState() });   // an overlay sidebar left open does not come back open later
const pageBack = () => $('main a[data-back]');
function barState() { $('#tbBack').disabled = navAt < 1 && !pageBack(); $('#tbFwd').disabled = navAt >= navEnd }
function bandState() {
  if (MAC) return;
  const s = $('#topSearch'), searchInBand = html.classList.contains('compact') && getComputedStyle(s).visibility !== 'hidden' && getComputedStyle(s).display !== 'none';
  const was = html.classList.contains('no-bar'), now = !$('#tbActs').children.length && !searchInBand;
  html.classList.toggle('no-bar', now);
  if (was !== now && window.onresize) window.onresize();   // a page that measures itself (the map) measures again
}
addEventListener('resize', () => requestAnimationFrame(bandState));
$('#tbBack').onclick = () => { const a = pageBack(); if (navAt > 0) history.back(); else if (a) location.hash = a.getAttribute('href') };
$('#tbFwd').onclick = () => history.forward();
function toggleSide() {
  if (wide.matches) { html.classList.toggle('side-off'); try { localStorage.setItem('sg.side-off', html.classList.contains('side-off') ? '1' : '0') } catch { } window.LB_LAYOUT() }
  else html.classList.toggle('side-open');
  railTitles(); bandState();
}
$('#sideBtn').onclick = toggleSide;
if (!MAC) {
  $('#sideBtn').before($('#tbBack'));   // drawn first, top left, so reached first
  // the full pane holds its search field above the rows; the folded rail has it in the command bar
  const placeSearch = () => {
    const f = $('#topSearch'), side = $('#side'), inPane = innerWidth >= 640 && !html.classList.contains('compact');
    if (inPane && f.parentElement !== side) $('.side-nav', side).before(f);
    else if (!inPane && f.parentElement === side) $('.top-in').append(f);
  };
  placeSearch(); new MutationObserver(placeSearch).observe(html, { attributes: true, attributeFilter: ['class'] }); addEventListener('resize', placeSearch);
}
document.addEventListener('click', e => { if (html.classList.contains('side-open') && !e.target.closest('.side,#sideBtn')) { html.classList.remove('side-open'); railTitles() } });
function toggleInsp() { html.classList.toggle('insp-off'); const d = $('.about-src', main); if (d && !html.classList.contains('insp-off')) d.open = true; store.set('insp-off', html.classList.contains('insp-off')) }
if (store.get('insp-off', true)) html.classList.add('insp-off');   // off until asked for: the page keeps the width
document.addEventListener('click', e => { if (e.target.closest('[data-insp]')) toggleInsp() });
$('#side').addEventListener('keydown', e => {
  if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
  const rows = $$('.side a.row-s').filter(a => a.offsetParent), i = rows.indexOf(document.activeElement);
  if (i < 0) return;
  e.preventDefault(); rows[(i + (e.key === 'ArrowDown' ? 1 : rows.length - 1)) % rows.length].focus();
});
const SECTIONS = ['#/videos', '#/map', '#/ask'];
const findBox = () => [$('#bigQ'), $('#searchQ'), $('#topQ')].find(x => x && x.offsetParent) || $('#chatIn');
document.addEventListener('keydown', e => {
  const typing = /INPUT|TEXTAREA|SELECT/.test(document.activeElement.tagName), cmd = MAC ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
  const is = c => e.code === 'Key' + c || (!e.code && (e.key || '').toUpperCase() === c);   // some input (assistive and remote tools) sends no key code
  if (e.key === '/' && !typing) { e.preventDefault(); findBox().focus() }
  else if (cmd && !e.altKey && !e.shiftKey && /^[1-3]$/.test(e.key)) { e.preventDefault(); location.hash = SECTIONS[e.key - 1] }
  else if (cmd && !e.altKey && !e.shiftKey && (is('F') || (!MAC && is('E')))) { e.preventDefault(); const f = findBox(); f.focus(); if (f.select) f.select() }
  else if (MAC && e.metaKey && e.ctrlKey && is('S')) { e.preventDefault(); toggleSide() }   // Windows apps have no key for this: the ☰ button
  else if (MAC ? e.metaKey && e.altKey && is('I') : e.ctrlKey && e.shiftKey && is('I')) { e.preventDefault(); if ($('.about-src', main)) toggleInsp() }
  else if (e.key === 'Escape') {
    if (html.classList.contains('side-open')) { html.classList.remove('side-open'); railTitles() }
    else if (document.activeElement === $('#topQ')) { $('#topQ').value = ''; $('#topQ').blur() }
  }
});
function notFound() { setTitle(t('app.notfound.title')); main.innerHTML = emptyState('search', t('app.notfound.head'), t('app.notfound.text')) }
const emptyState = (icon, title, text) => `<div class="empty"><span class="e-ico">${ico(icon)}</span><b>${esc(title)}</b><p>${esc(text)}</p><p class="e-act"><a class="btn plain" href="#/">${th('app.empty.go_videos')}</a></p></div>`;
function searchBox(id, value, placeholder, button = t('app.search.button')) {
  return `<form class="bigsearch" role="search" id="${id}F">${ico('search')}<label class="sr" for="${id}">${th('app.search.label')}</label><input id="${id}" type="search" autocomplete="off" value="${esc(value || '')}" placeholder="${esc(placeholder)}" enterkeyhint="search"><button>${esc(button)}</button></form>`;
}

/* --------------------------------------------------------------- home -- */
// Example searches under the home page's field, in the interface's language
const HOME_EXAMPLES = ['app.home.ex_boil', 'app.home.ex_bleeding', 'app.home.ex_fridge', 'app.home.ex_burn', 'app.home.ex_toilet', 'app.home.ex_gas'];
async function home() {
  setTitle('');
  const tile = topic => `<a class="need" href="#/t/${topic.id}" ${tcStyle(topic.id)}><span class="ico">${ico(topic.id)}</span><b>${esc(shortOf(topic.id, topic.title))}</b><small>${esc(topic.short || '')}</small></a>`;
  const cont = await continueItems();
  const pins = (await libInfo()).pins !== false;
  main.innerHTML = `
  <section class="hero">
    <h1>${th('app.home.title')}</h1>
    <p class="lead">${th('app.home.lead')}</p>
    ${searchBox('bigQ', '', innerWidth < 500 ? t('app.home.ph_short') : t('app.home.ph'))}
    <div class="chips examples" aria-label="${th('app.home.examples')}">${HOME_EXAMPLES.map(k => t(k)).map(x => `<a class="chip" href="#/search?q=${encodeURIComponent(x)}">${esc(x)}</a>`).join('')}</div>
  </section>
  ${secHead(th('app.home.now'))}
  <div class="needs">${nowTopics().map(tile).join('')}</div>
  <div class="banners"><a class="banner chatb" href="#/ask"><span class="ico">${ico('ask')}</span><span><b>${th('app.home.ask')}</b><small>${th('app.home.ask_sub')}</small></span><span class="go">${ico('right')}</span></a>
  <a class="banner map" href="#/map"><span class="ico">${ico('pin')}</span><span>${pins ? `<b>${th('app.home.map')}</b><small>${th('app.home.map_sub')}</small>` : `<b>${th('app.home.place')}</b><small>${th('app.home.place_sub')}</small>`}</span><span class="go">${ico('right')}</span></a></div>
  ${cont.length ? secHead(th('app.home.continue')) + rail(cont.join('')) : ''}
  <div id="homeVideos"></div>
  ${secHead(th('app.learn.title'), th('app.home.learn_sub'), `<a href="#/library">${th('app.home.all_skills')}</a>`)}
  ${rail(learnTopics().map(topic => `<a class="skill" href="#/t/${topic.id}" ${tcStyle(topic.id)}><span class="ico">${ico(topic.id)}</span><b>${esc(shortOf(topic.id, topic.title))}</b><small>${esc(topic.short || '')}</small><span class="steps">${th('app.learn.steps', { n: (topic.route || []).length })}</span></a>`).join(''), 'skills')}
  <a class="banner prep" href="#/prepare"><span class="ico">${ico('prep')}</span><span><b>${th('app.prep.banner')}</b><small>${th('app.home.prep_sub')}</small></span><span class="go">${ico('right')}</span></a>`;
  $('#bigQF').addEventListener('submit', e => { e.preventDefault(); location.hash = '#/search?q=' + encodeURIComponent($('#bigQ').value.trim()) });
  const ex = $('.examples', main), exEnd = () => { ex.classList.toggle('at-end', ex.scrollLeft + ex.clientWidth >= ex.scrollWidth - 4); ex.classList.toggle('at-start', ex.scrollLeft < 4) };
  ex.addEventListener('scroll', exEnd, { passive: true }); exEnd(); window.onresize = exEnd;
  try {
    const rows = await api('/api/videos');
    const g = rows.find(r => r.id === 'guides');
    if (g && $('#homeVideos')) { $('#homeVideos').innerHTML = secHead(th('app.home.watch'), th('app.home.watch_sub'), `<a href="#/videos">${th('app.home.all_videos')}</a>`) + rail(g.items.slice(0, 14).map(v => vcard(v, { style: tcStyle(v.guide) })).join('')); wireRails($('#homeVideos')) }
  } catch { }
}
async function continueItems() {
  const out = [], rec = await recents();
  (await resumable(12)).slice(0, 6)
    .forEach(([id, v]) => out.push(vcard({ id, title: v.title, thumb: v.thumb, duration: v.d }, { sub: t('app.video.resume_at', { time: clock(v.t) }) })));
  rec.filter(r => !r.key.startsWith('v:')).slice(0, 8).forEach(r => out.push(`<a class="mini" href="${esc(r.href)}"><small>${esc(r.sub || '')}</small><b>${esc(r.title)}</b></a>`));
  return out;
}

/* -------------------------------------------------------------- guide -- */
function topicVideos(t) {
  const all = [...(t.choices || []).flatMap(c => c.read || []), ...(t.route || []).flatMap(s => s.read || []), ...(t.learn || []), ...(t.prepare || [])];
  const seen = new Set();
  return all.filter(i => i.src && i.src.kind === 'video' && !seen.has(i.src.id) && seen.add(i.src.id)).map(i => i.src);
}
async function topicPage(id, open) {
  const topic = topicById(id);
  if (!topic) return notFound();
  setTitle(topic.title);
  remember({ key: 't:' + id, title: topic.title, href: '#/t/' + id, sub: topic.mode === 'learn' ? t('app.topic.route_sub') : t('app.topic.guide_sub') });
  const learn = topic.mode === 'learn';
  const progress = store.get('progress', {});
  const sits = (topic.choices || []).map(c => c.goto
    ? `<div class="sit"><a href="#/t/${esc(c.goto)}"><span class="dot"></span><span class="tx"><b>${esc(c.label)}</b>${c.hint ? `<span>${esc(c.hint)}</span>` : ''}</span><span class="chev">${ico('right')}</span></a></div>`
    : `<div class="sit${open === c.id ? ' open' : ''}" id="c-${esc(c.id)}">
      <button aria-expanded="${open === c.id}" aria-controls="cb-${esc(c.id)}" data-choice="${esc(c.id)}"><span class="dot"></span><span class="tx"><b>${esc(c.label)}</b>${c.hint ? `<span>${esc(c.hint)}</span>` : ''}</span><span class="chev">${ico('down')}</span></button>
      <div class="body" id="cb-${esc(c.id)}"><div class="rcards">${(c.read || []).map(rcard).join('') || `<p class="muted">${th('app.topic.nothing')}</p>`}</div></div></div>`).join('');
  const limLi = l => `<li>${esc(l.text)}${l.src ? ` <span class="cite">— <a href="${hrefFor(l)}">${esc(l.src.publisher || l.src.title)}${l.page ? ', ' + th('app.src.p', { page: l.page }) : ''}</a></span>` : l.zim ? ` <span class="cite">— <a href="${hrefFor(l)}">${th('app.topic.encyclopedia')}</a></span>` : ''}</li>`;
  const LIM = 4, lims = topic.limits || [];
  const limits = lims.length ? `<section class="limits" aria-labelledby="lim"><h2 id="lim">${ico('warn')}${th('app.topic.limits')}</h2><ul>${lims.slice(0, LIM).map(limLi).join('')}</ul>${lims.length > LIM ? `<details><summary>${th('app.topic.all_warnings', { n: lims.length })}</summary><ul>${lims.slice(LIM).map(limLi).join('')}</ul></details>` : ''}</section>` : '';
  const disagree = (topic.gaps?.disagree || []).length ? `<div class="disagree"><b>${th('app.topic.disagree')}</b><ul>${topic.gaps.disagree.map(x => `<li>${esc(x)}</li>`).join('')}</ul></div>` : '';
  const route = (topic.route || []).length ? `<h2>${th('app.topic.route')}</h2><p class="muted small">${th('app.topic.route_note')}</p>
    <ol class="route">${topic.route.map((s, i) => { const k = id + ':' + i, p = progress[k] || {}; return `<li class="step${p.practised ? ' done' : ''}"><h3>${esc(s.title.replace(/^\p{L}+\s\d+\s*:\s*/u, ''))}</h3>${s.goal ? `<p class="goal">${esc(s.goal)}</p>` : ''}
      <div class="rcards">${(s.read || []).map(rcard).join('')}</div>
      <div class="mark"><label><input type="checkbox" data-prog="${k}" data-f="read" ${p.read ? 'checked' : ''}> ${th('app.topic.read')}</label><label><input type="checkbox" data-prog="${k}" data-f="practised" ${p.practised ? 'checked' : ''}> ${th('app.topic.practised')}</label></div></li>` }).join('')}</ol>` : '';
  const sec = (icon, title, items) => items && items.length ? `<details class="more"><summary>${ico(icon)}<span>${title} <span class="n">· ${items.length}</span></span></summary><div class="body"><ul class="list">${items.map(i => rowItem({ ...i, preview: true })).join('')}</ul></div></details>` : '';
  const vids = topicVideos(topic);
  const related = (topic.related || []).map(topicById).filter(Boolean);
  main.innerHTML = `<div class="ghero" ${tcStyle(id)}><div class="ghero-in">${crumbs([[t('nav.home'), '#/'], ...(learn ? [[t('app.learn.title'), '#/library']] : []), [shortOf(id, topic.title)]])}
      <div class="row1"><span class="ico">${ico(topic.id)}</span><h1>${esc(topic.title)}</h1></div><p class="lead">${esc(topic.intro || '')}</p>
      <div class="vactions" style="margin-bottom:0">${askLink('topic:' + id, t('app.topic.ask'))}</div></div></div>
    <div class="gcols" ${tcStyle(id)}><div>
      ${(topic.choices || []).length ? `<p class="ask" id="askh">${learn ? th('app.topic.start') : th('app.topic.happening')}</p><div class="situations" aria-labelledby="askh">${sits}</div>` : ''}
      <div class="gnarrow">${limits}${disagree}</div>
      ${route}
      ${vids.length ? secHead(th('app.topic.videos'), th('app.topic.picked', { n: vids.length })) + rail(vids.map(v => vcard(v)).join('')) : ''}
      ${sec('prep', th('app.prep.banner'), topic.prepare)}
      ${sec('book', th('app.topic.deeper'), topic.learn)}
      ${(topic.gaps?.missing || []).length ? `<details class="more"><summary>${ico('warn')}<span>${th('app.topic.missing')}</span></summary><div class="body"><p class="muted small" style="margin-top:0">${th('app.topic.missing_note')}</p><ul>${topic.gaps.missing.map(x => `<li>${esc(x)}</li>`).join('')}</ul></div></details>` : ''}
      ${related.length ? `<h2>${th('app.topic.related')}</h2><div class="related">${related.map(r => `<a class="chip" href="#/t/${r.id}" ${tcStyle(r.id)}><span style="color:var(--tc)">${ico(r.id)}</span>${esc(shortOf(r.id, r.title))}</a>`).join('')}</div>` : ''}
    </div><aside class="gside" aria-label="${th('app.topic.safety')}"></aside></div>`;
  // On wide screens the safety box sits beside the choices so it stays visible.
  const side = $('.gside', main), nar = $('.gnarrow', main);
  const placeSide = () => { if (!side || !nar) return; if (innerWidth >= 1050) { if (!side.firstChild) side.append(...nar.childNodes) } else if (side.firstChild) nar.append(...side.childNodes) };
  placeSide(); window.onresize = placeSide;
  $$('[data-choice]', main).forEach(b => b.addEventListener('click', () => {
    const c = b.closest('.sit'), on = !c.classList.contains('open');
    $$('.sit.open', main).forEach(x => { x.classList.remove('open'); $('button', x).setAttribute('aria-expanded', 'false') });
    if (on) { c.classList.add('open'); b.setAttribute('aria-expanded', 'true') }
    history.replaceState(null, '', '#/t/' + id + (on ? '/' + b.dataset.choice : ''));
    if (on) setTimeout(() => c.scrollIntoView({ block: 'start', behavior: 'smooth' }), 30);
  }));
  $$('[data-prog]', main).forEach(cb => cb.addEventListener('change', () => {
    const p = store.get('progress', {}); p[cb.dataset.prog] = { ...(p[cb.dataset.prog] || {}), [cb.dataset.f]: cb.checked }; store.set('progress', p);
    cb.closest('.step').classList.toggle('done', !!p[cb.dataset.prog].practised);
  }));
  if (open) { const c = $('#c-' + CSS.escape(open)); if (c) setTimeout(() => c.scrollIntoView({ block: 'start' }), 0) }
}

/* ------------------------------------------------------------- videos -- */
// Video lengths a list can be cut to: the address's word, and its name
const LENGTHS = [['short', 'app.videos.short'], ['medium', 'app.videos.medium'], ['long', 'app.videos.long']];
async function videosHome() {
  setTitle(t('nav.videos'));
  main.innerHTML = `<h1>${th('nav.videos')}</h1><p class="lead">${th('app.videos.lead')}</p>
    <div class="vtools">${searchBox('vq', '', t('app.videos.search_titles'))}</div>
    <div class="chips" style="margin-top:8px">${LENGTHS.map(([k, l]) => `<a class="chip" href="#/videos/all?len=${k}">${ico('clock')}${th(l)}</a>`).join('')}</div>
    <div id="vrows"><p class="loading">${th('app.videos.loading')}</p></div>`;
  $('#vqF').addEventListener('submit', e => { e.preventDefault(); location.hash = '#/videos/all?q=' + encodeURIComponent($('#vq').value.trim()) });
  const rows = await api('/api/videos');
  const cont = (await resumable(20)).slice(0, 12);
  const GCOL = { health: 'hurt', water: 'water', food: 'food', power: 'power', outdoors: 'shelter', repair: 'repair', making: 'land' };
  $('#vrows').innerHTML =
    (cont.length ? secHead(th('app.videos.continue')) + rail(cont.map(([id, v]) => vcard({ id, title: v.title, thumb: v.thumb, duration: v.d }, { sub: t('app.video.resume_at', { time: clock(v.t) }) })).join('')) : '') +
    rows.map(r => secHead(esc(rowName(r)), esc(rowNote(r)), `<a href="#/videos/${r.id}">${th('app.see_all', { count: r.count })}</a>`) +
      rail(r.items.map(v => vcard(v, { style: tcStyle(v.guide || GCOL[r.id]) })).join(''))).join('') +
    `<p class="small muted" style="margin-top:24px">${th('app.videos.note')}</p>`;
  wireRails();
}
async function videoRow(row, q) {
  const len = q.get('len') || '', sort = q.get('sort') || '', qq = q.get('q') || '';
  const base = `#/videos/${row}`;
  const qs = o => { const p = new URLSearchParams({ q: qq, len, sort, ...o }); for (const [k, v] of [...p]) if (!v) p.delete(k); const s = p.toString(); return base + (s ? '?' + s : '') };
  let off = 0;
  const first = await api(`/api/videos/${row}?q=${encodeURIComponent(qq)}&len=${len}&sort=${sort}&offset=0`);
  const title = rowName({ id: first.id ?? row, title: first.title }), note = rowNote(first);   // an older engine sends no id: the address names the row
  setTitle(title);
  main.innerHTML = `<div>${backButton('#/videos', t('nav.videos'))}</div><h1 style="margin-top:12px">${esc(qq ? t('app.videos.query', { q: qq }) : title)}</h1>${note ? `<p class="lead">${esc(note)}</p>` : ''}
    <div class="vtools">${searchBox('vq', qq, t('app.videos.search_these'), t('app.videos.go'))}
      <label class="sr" for="vsort">${th('app.videos.sort')}</label><select id="vsort">${[['', 'app.videos.sort_useful'], ['short', 'app.videos.sort_short'], ['new', 'app.videos.sort_new'], ['az', 'app.videos.sort_az']].map(([k, l]) => `<option value="${k}" ${k === sort ? 'selected' : ''}>${th(l)}</option>`).join('')}</select></div>
    <div class="chips" style="margin:8px 0 16px">${[['', 'app.videos.any'], ...LENGTHS].map(([k, l]) => `<a class="chip ${k === len ? 'on' : ''}" href="${qs({ len: k })}">${th(l)}</a>`).join('')}</div>
    <p class="muted small">${th('app.videos.count', { n: first.total })}</p>
    <div class="vgrid" id="vg"></div><p id="vmore"></p>`;
  $('#vqF').addEventListener('submit', e => { e.preventDefault(); location.hash = qs({ q: $('#vq').value.trim() }) });
  $('#vsort').addEventListener('change', e => { location.hash = qs({ sort: e.target.value }) });
  const add = d => {
    $('#vg').insertAdjacentHTML('beforeend', d.items.map(v => vcard(v)).join('') || (off ? '' : `<div class="empty" style="grid-column:1/-1">${th('app.videos.none')}</div>`));
    $('#vmore').innerHTML = d.more ? `<button class="btn plain" id="vmb">${th('app.videos.more')}</button>` : '';
    if (d.more) $('#vmb').onclick = async () => { off += 48; add(await api(`/api/videos/${row}?q=${encodeURIComponent(qq)}&len=${len}&sort=${sort}&offset=${off}`)) };
  };
  add(first);
}

// Video formats some computers can't play, by the code an MP4 file gives its video track: the name to show, and what to ask the player.
const UNPLAYABLE = { av01: ['AV1', 'video/mp4; codecs="av01.0.05M.08"'], vp09: ['VP9', 'video/mp4; codecs="vp09.00.10.08"'], hvc1: ['HEVC', 'video/mp4; codecs="hvc1.1.6.L93.B0"'], hev1: ['HEVC', 'video/mp4; codecs="hev1.1.6.L93.B0"'] };
// The format codes of an MP4 file's tracks ("av01", "mp4a"), read from the file's own index. [] when they can't be read (not an MP4, say).
// An MP4 is a row of boxes, each starting with its length and a four-letter name. This reads the start of each box until it finds
// the index ("moov"), then reads only that: never the pictures or the sound.
async function videoFormats(url) {
  let size = 0;
  const part = async (at, n) => {
    const r = await fetch(url, { headers: { Range: `bytes=${at}-${at + n - 1}` } });
    size = +(/\/(\d+)$/.exec(r.headers.get('Content-Range') || '') || [])[1] || 0;
    if (r.status !== 206 || !size) throw new Error('no byte range');
    return new DataView(await r.arrayBuffer());
  };
  const name = (v, at) => String.fromCharCode(v.getUint8(at), v.getUint8(at + 1), v.getUint8(at + 2), v.getUint8(at + 3));
  const found = [];
  const inside = (v, at, end) => {   // the boxes between at and end; a track's format is the first name in its "stsd" box
    while (at + 8 <= end) {
      let n = v.getUint32(at), head = 8;
      if (n === 1 && at + 16 <= end) { n = Number(v.getBigUint64(at + 8)); head = 16 } else if (n === 0) n = end - at;
      if (n < head || at + n > end) return;
      const type = name(v, at + 4);
      if (type === 'stsd' && n >= head + 16) found.push(name(v, at + head + 12));
      else if (['moov', 'trak', 'mdia', 'minf', 'stbl'].includes(type)) inside(v, at + head, at + n);
      at += n;
    }
  };
  for (let at = 0, i = 0; i < 64; i++) {
    const v = await part(at, 16);
    if (v.byteLength < 16) break;
    let n = v.getUint32(0);
    if (n === 1) n = Number(v.getBigUint64(8)); else if (n === 0) n = size - at;
    if (n < 8) break;
    if (name(v, 4) === 'moov') { if (n <= 32 * 1024 * 1024) { const m = await part(at, n); inside(m, 0, m.byteLength) } break }
    at += n;
    if (at + 16 > size) break;
  }
  return found;
}

// A day as the language writes it, from 20230514 or 2023-05-14(T…); anything else as it came
const dayOf = s => { const m = /^(\d{4})-?(\d\d)-?(\d\d)/.exec(String(s || '')); return m ? I18N.date(new Date(+m[1], +m[2] - 1, +m[3]), { year: 'numeric', month: 'short', day: 'numeric' }) : String(s || '') };
async function watch(id, at, from) {
  const d = await api('/api/doc/' + encodeURIComponent(id));
  setTitle(d.title);
  const selfHref = `#/watch/${encodeURIComponent(id)}`;
  remember({ key: 'v:' + id, title: d.title, href: selfHref, sub: t('app.video.sub', { dur: fmtDur(d.duration) }) });
  const p = vprog()[id];
  const resumeAt = !at && p && p.t > 15 && (!d.duration || p.t < d.duration - 20) ? p.t : 0;
  main.innerHTML = `<div class="rtop">${backButton(from, t('nav.videos'))}</div>
  <div class="watch">
    <div>
      <div class="player"><video id="vid" controls preload="metadata" playsinline src="${fileUrl(d.path)}${at ? '#t=' + at : ''}"></video></div>
      ${d.remote ? `<p class="small muted" style="margin:6px 0 0">${th('app.video.remote', { name: d.remote })}</p>` : ''}
      <h1 class="vtitle">${esc(d.title)}</h1>
      <div class="vmeta"><b>${esc(d.publisher || '')}</b><span>${esc(fmtDur(d.duration))}</span>${d.upload_date ? `<span>${esc(dayOf(d.upload_date))}</span>` : ''}<span>${esc(trustOf(d))}</span></div>
      ${resumeAt ? `<div class="resume" id="resume">${th('app.video.stopped_at', { time: clock(resumeAt) })} <button class="btn" id="resYes">${th('app.video.resume')}</button><button class="btn plain" id="resNo">${th('app.video.start_over')}</button></div>` : ''}
      <div class="vactions dock">${saveButton({ key: 'v:' + id, title: d.title, href: selfHref, sub: t('app.video.sub', { dur: fmtDur(d.duration) }) })}<a class="btn plain" title="${th('app.video.ask')}" href="#/ask?about=${encodeURIComponent('video:' + id)}&${fromQ()}" id="askv">${ico('ask')}<span>${th('app.video.ask')}</span></a></div>
      <div class="notice small">${th('app.video.notice')}</div>
    </div>
    <section class="txpanel" id="tx" aria-label="${th('app.video.transcript')}"><header><h2>${th('app.video.transcript')}</h2><label class="sr" for="cueQ">${th('app.video.find_label')}</label><input id="cueQ" type="search" placeholder="${th('app.video.find_ph')}"></header>
      <div class="cues" id="cues"><p class="muted" style="padding:12px 14px">${th('app.video.loading_tx')}</p></div><div class="txnote" id="txnote"></div></section>
  </div>
  <div id="morev"></div>
  <details class="about-src"><summary>${th('app.video.about')}</summary><div class="in"><dl><dt>${th('app.video.channel')}</dt><dd>${esc(d.publisher || '—')}</dd><dt>${th('app.src.kind')}</dt><dd>${esc(trustOf(d))}</dd><dt>${th('app.video.length')}</dt><dd>${esc(fmtDur(d.duration)) || th('app.video.unknown')}</dd>${d.notes ? `<dt>${th('app.src.note')}</dt><dd>${esc(d.notes)}</dd>` : ''}${d.source_url ? `<dt>${th('app.src.original')}</dt><dd>${esc(hostOf(d.source_url))} <span class="muted">${th('app.video.online_note')}</span></dd>` : ''}</dl></div></details>`;
  const vid = $('#vid');
  if (d.thumb) vid.poster = fileUrl(d.thumb);
  vid.addEventListener('error', async () => {
    const say = why => vid.closest('.player')?.insertAdjacentHTML('afterend', `<div class="err">${why}</div>`);
    if (d.remote) return say(th('app.video.remote_down', { name: d.remote }));
    if (!(await away.here())) return;   // the library is out of reach: the banner says so
    // Say why when the reason is the format: some library videos are AV1, which a computer without an AV1 decoder (a Mac before the M3) can't play.
    const f = (await videoFormats(fileUrl(d.path)).catch(() => [])).map(x => UNPLAYABLE[x]).find(x => x && !vid.canPlayType(x[1]));
    if (!vid.isConnected) return;   // the person has gone to another page
    say(f ? th('app.video.format', { format: f[0], path: I18N.raw(`<b style="overflow-wrap:anywhere">library-data/${esc(d.path)}</b>`) })
      : th('app.video.cant_play', { link: I18N.raw(`<a href="${fileUrl(d.path)}" target="_blank" rel="noopener">${th('app.video.open_file')}</a>`) }));
  }, { once: true });
  if (resumeAt) {
    $('#resYes').onclick = () => { vid.currentTime = resumeAt; vid.play().catch(() => { }); $('#resume').remove() };
    $('#resNo').onclick = () => { vid.currentTime = 0; vid.play().catch(() => { }); $('#resume').remove() };
  }
  let lastSave = 0;
  const saveProg = () => { if (vid.currentTime > 3) setVprog(id, { t: vid.currentTime, d: d.duration || vid.duration, title: d.title, thumb: d.thumb }) };
  vid.addEventListener('timeupdate', () => { if (Date.now() - lastSave > 4000) { lastSave = Date.now(); saveProg() } });
  vid.addEventListener('pause', saveProg);
  leaving = () => { saveProg(); vid.pause() };
  // keep "Ask about this video" pointing at the current moment
  vid.addEventListener('pause', () => { const a = $('#askv'); if (a) a.href = `#/ask?about=${encodeURIComponent('video:' + id + ':' + Math.round(vid.currentTime))}&${fromQ()}` });
  let cues = [];
  try {
    const tx = await api('/api/transcript/' + encodeURIComponent(id)); cues = tx.cues || [];
    $('#txnote').textContent = cues.length ? t('app.video.captions_note', { source: t(tx.source === 'authored' ? 'app.video.captions_author' : /auto/.test(tx.source || '') ? 'app.video.captions_auto' : 'app.video.captions') }) : '';
  } catch { }
  const box = $('#cues');
  const draw = q => {
    const rx = q ? new RegExp(q.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'ig') : null;
    const list = cues.filter(c => !rx || c.text.match(rx));
    box.innerHTML = list.length ? list.map(c => `<button class="cue" data-t="${c.t ?? ''}"><time>${c.t != null ? clock(c.t) : ''}</time><span>${rx ? esc(c.text).replace(rx, m => `<mark>${m}</mark>`) : esc(c.text)}</span></button>`).join('')
      : `<p class="muted" style="padding:12px 14px">${cues.length ? th('app.video.no_match') : th('app.video.no_tx')}</p>`;
  };
  draw('');
  $('#cueQ').addEventListener('input', e => draw(e.target.value.trim()));
  box.addEventListener('click', e => { const b = e.target.closest('.cue'); if (b && b.dataset.t !== '') { vid.currentTime = +b.dataset.t; vid.play().catch(() => { }); if (innerWidth < 1000) scrollTo({ top: 0, behavior: 'smooth' }) } });
  let lastOn = null;
  vid.addEventListener('timeupdate', () => {
    const now = vid.currentTime; let on = null;
    for (const b of box.children) { if (b.dataset && b.dataset.t !== '' && +b.dataset.t <= now) on = b; else if (on) break }
    if (on !== lastOn) { lastOn && lastOn.classList.remove('on'); on && on.classList.add('on'); lastOn = on; if (on && !$('#cueQ').value && innerWidth >= 1000) on.scrollIntoView({ block: 'nearest' }) }
  });
  try {
    const rel = await api('/api/videos/related/' + encodeURIComponent(id));
    if (rel.length && $('#morev')) { $('#morev').innerHTML = secHead(th('app.video.more')) + rail(rel.map(v => vcard(v)).join('')); wireRails($('#morev')) }
  } catch { }
}

/* ------------------------------------------------------------- reader -- */
async function reader(id, page, from, view) {
  const d = await api('/api/doc/' + encodeURIComponent(id));
  if (d.kind === 'video') { swap('#/watch/' + encodeURIComponent(id) + (from ? '?from=' + encodeURIComponent(from) : '')); return }
  if (d.kind === 'transcript' && d.videoId) { swap('#/watch/' + encodeURIComponent(d.videoId) + (from ? '?from=' + encodeURIComponent(from) : '')); return }
  setTitle(d.title, [d.publisher, d.year, trustOf(d)].filter(Boolean).join(' · '));
  const isPdf = d.kind === 'pdf', total = d.pages || 1;
  const isWeb = d.kind === 'html' && /\.x?html?$/i.test(d.path);   // a plain text file, such as a lecture transcript, reads as text in the page itself
  page = Math.min(Math.max(page || 1, 1), total);
  view = isPdf ? view || 'page' : isWeb ? view || 'web' : 'text';
  const key = 'd:' + id + (isPdf ? ':' + page : '');
  const selfHref = `#/read/${encodeURIComponent(id)}?p=${page}`;
  remember({ key: 'd:' + id, title: d.title, href: selfHref, sub: (isPdf ? t('app.reader.page_n', { page }) + ' · ' : '') + (d.publisher || '') });
  const go = (p, v = view) => `#/read/${encodeURIComponent(id)}?p=${p}&view=${v}${from ? '&from=' + encodeURIComponent(from) : ''}`;
  const views = isPdf ? [['page', 'app.reader.view_page'], ['text', 'app.reader.view_text'], ['pdf', 'app.reader.view_pdf']] : isWeb ? [['web', 'app.reader.view_page'], ['text', 'app.reader.view_text_only']] : [];
  main.innerHTML = `
    <div class="rtop">${backButton(from)}</div>
    <h1 class="rtitle">${esc(d.title)}</h1>
    <div class="rsrc"><b>${esc(d.publisher || '')}</b>${d.year ? ' · ' + esc(d.year) : ''} · ${esc(trustOf(d))}</div>
    <div class="rmodes">${views.length ? `<div class="seg dock" role="group" aria-label="${th('app.reader.views')}">${views.map(([v, l]) => `<button data-view="${v}" aria-pressed="${v === view}">${th(l)}</button>`).join('')}</div>` : ''}<span class="vactions dock">${askLink('doc:' + id + ':' + (isPdf ? page : 1), t('app.ask.short'))}${saveButton({ key, title: isPdf ? t('app.reader.title_page', { title: d.title, page }) : d.title, href: selfHref, sub: d.publisher || kindOf(d) })}<button class="tb-btn insp-btn" data-insp aria-label="${th('app.reader.details_toggle')}" title="${th('app.reader.details')}">${ico('insp')}</button></span></div>
    <div id="docbody"></div>
    ${d.imageOnly ? `<div class="notice">${th('app.reader.scanned')}</div>` : ''}
    ${isPdf && view !== 'pdf' ? `<div class="rbar"><a class="btn plain nav" href="${go(page - 1)}" ${page <= 1 ? 'aria-disabled="true" style="opacity:.35;pointer-events:none"' : ''} aria-label="${th('app.reader.prev_page')}">${ico('back')}<span>${th('app.reader.prev')}</span></a>
      <span class="pnum"><label class="sr" for="pg">${th('app.reader.page_number')}</label>${th('app.reader.page_of', { input: I18N.raw(`<input id="pg" inputmode="numeric" value="${page}">`), total })}</span>
      <a class="btn nav" href="${go(page + 1)}" ${page >= total ? 'aria-disabled="true" style="opacity:.35;pointer-events:none"' : ''} aria-label="${th('app.reader.next_page')}"><span>${th('app.reader.next')}</span>${ico('right')}</a></div>` : ''}
    <details class="about-src"><summary>${th('app.src.about')}</summary><div class="in"><p class="insp-h">${esc(d.title)}</p>
      <dl><dt>${th('app.src.publisher')}</dt><dd>${esc(d.publisher || '—')}</dd><dt>${th('app.src.kind')}</dt><dd>${esc(trustOf(d))} · ${esc(kindOf(d))}</dd>
      ${d.edition ? `<dt>${th('app.src.edition')}</dt><dd>${esc(d.edition)}</dd>` : ''}${d.year ? `<dt>${th('app.src.year')}</dt><dd>${esc(d.year)}</dd>` : ''}
      <dt>${th('app.src.saved')}</dt><dd>${d.retrieved_at ? esc(dayOf(d.retrieved_at)) : th('app.src.unknown_date')} · ${esc(d.collection)}</dd>
      ${d.notes ? `<dt>${th('app.src.note')}</dt><dd>${esc(d.notes)}</dd>` : ''}
      ${d.source_url ? `<dt>${th('app.src.original')}</dt><dd>${esc(hostOf(d.source_url))} <span class="muted">${th('app.src.online_note')}</span></dd>` : ''}</dl>
      <p class="small muted">${th('app.src.reference')}</p>
      ${isPdf || isWeb || !IN_WINDOW ? `<p><a class="btn ghost" href="${fileUrl(d.path)}${isPdf ? '#page=' + page : ''}" target="_blank" rel="noopener">${ico('ext')}${th('app.src.open_original')}</a></p>` : ''}</div></details>`;
  $$('[data-view]', main).forEach(b => b.addEventListener('click', () => swap(go(page, b.dataset.view))));
  if (innerWidth >= 1060 && !html.classList.contains('insp-off')) $('.about-src', main).open = true;
  const pg = $('#pg'); if (pg) pg.addEventListener('change', () => { const n = parseInt(pg.value, 10); if (n >= 1 && n <= total) location.hash = go(n) });
  const body = $('#docbody');
  if (view === 'page') {
    const w = Math.min(1600, Math.round(Math.min(innerWidth, 1000) * Math.min(2, devicePixelRatio || 1)));
    body.innerHTML = `<div class="pagewrap"><img class="pageimg" alt="${th('app.reader.page_alt', { page, title: d.title })}" src="${pageImg(id, page, w)}"></div>`;
    const img = $('img', body);
    img.addEventListener('error', async () => { if (await away.here()) body.innerHTML = `<div class="notice">${th('app.reader.no_image', { link: I18N.raw(`<a href="${go(page, 'text')}">${th('app.reader.show_text')}</a>`) })}</div>` });   // the library out of reach: the banner says so
    if (page < total) { const n = new Image(); n.src = pageImg(id, page + 1, w) }
    let sx = null, sy = 0;
    const wrap = $('.pagewrap', body);
    wrap.addEventListener('touchstart', e => { if (e.touches.length === 1) { sx = e.touches[0].clientX; sy = e.touches[0].clientY } }, { passive: true });
    wrap.addEventListener('touchend', e => { if (sx == null) return; const dx = e.changedTouches[0].clientX - sx, dy = e.changedTouches[0].clientY - sy; sx = null; if (Math.abs(dx) > 70 && Math.abs(dx) > Math.abs(dy) * 1.5) { if (dx < 0 && page < total) location.hash = go(page + 1); if (dx > 0 && page > 1) location.hash = go(page - 1) } }, { passive: true });
  } else if (view === 'pdf') {
    body.innerHTML = `<iframe class="frame" title="${esc(d.title)}" src="${fileUrl(d.path)}#page=${page}"></iframe><p class="small muted">${th('app.reader.pdf_note')}</p>`;
  } else if (view === 'web') {
    const own = /(^|\/)documents\/lastbook\//.test(d.path);   // Lastbook's own pages (Sources and permissions) take the window's look from the server (crates/engine/src/library/saved_pages.rs, OWN_LOOK); an older server's stays a white sheet
    body.innerHTML = `<iframe class="frame${own ? ' own' : ''}" title="${esc(d.title)}" src="${fileUrl(d.path)}"></iframe>`;
  } else {
    body.innerHTML = `<p class="loading">${th('app.reader.loading_text')}</p>`;
    try {
      const got = await api(`/api/page/${encodeURIComponent(id)}/${isPdf ? page : 1}`);
      const txt = (got.text || '').replace(/[ \t]{2,}/g, ' ').replace(/\n{3,}/g, '\n\n').trim();
      // a picture's description, which the library's tools take from an accessible PDF, marked "[Alt text: …]" in the data
      const shown = esc(txt).replace(/^\[Alt text: ([\s\S]*?)\]$/gm, (_, s) => `<span class="alttext"><b>${th('app.reader.alt_text')}</b> ${s}</span>`);
      body.innerHTML = `${isPdf || isWeb ? `<div class="notice small">${th('app.reader.extracted')}</div>` : ''}<div class="pagetext">${shown || `<span class="muted">${th('app.reader.no_text')}</span>`}</div>`;
    } catch (e) { body.innerHTML = errorBox(e, t('app.reader.text_failed')) }
  }
  main.onkeydown = e => { if (!isPdf || /INPUT/.test(e.target.tagName) || e.altKey || e.metaKey || e.ctrlKey) return; if (e.key === 'ArrowRight' && page < total) location.hash = go(page + 1); if (e.key === 'ArrowLeft' && page > 1) location.hash = go(page - 1) };
}

/* --------------------------------------------------------------- wiki -- */
const ZIMNAMES = { mdwiki: 'app.zim.mdwiki_label', wikipedia: 'app.zim.wikipedia_label', wikibooks: 'app.zim.wikibooks_label', appropedia: 'app.zim.appropedia_label' };
const zimLabel = name => { for (const k in ZIMNAMES) if (name.startsWith(k)) return t(ZIMNAMES[k]); return name };
async function wiki(path, from, y) {
  const [zim, ...rest] = path.split('/');
  const art = rest.join('/');
  const title = decodeURIComponent(art.split('/').pop() || '').replace(/_/g, ' ') || zimLabel(zim);
  setTitle(title, '', zimLabel(zim));
  const src = '/zim/' + encodeURIComponent(zim) + '/' + art.split('/').map(encodeURIComponent).join('/');
  const selfHref = '#/wiki/' + path;
  const mainPage = zimMain(zim);
  main.innerHTML = `<div class="rtop">${backButton(from || '#/encyclopedias')}<span id="wsave" class="dock">${saveButton({ key: 'w:' + path, title, href: selfHref, sub: zimLabel(zim) })}</span></div>
    <h1 class="rtitle" id="wtitle">${esc(title)}</h1><div class="rsrc"><span class="zn">${esc(zimLabel(zim))} · </span>${th('app.wiki.note')}${IN_WINDOW ? '' : ` · <a id="wopen" href="${src}" target="_blank" rel="noopener">${th('app.wiki.open_own')}</a>`}</div>
    <iframe class="frame" id="wframe" title="${esc(title)}" src="${src}"></iframe>`;
  const f = $('#wframe');
  // The toolbar names the encyclopedia, and the article once the article's own title has scrolled up out of the frame, as a large title does
  const barName = () => { const n = f.lbName, b = $('#tbTitle'), t = n && f.contentWindow.scrollY > n.at - 4 ? n.nt : zimLabel(zim); if (b.textContent !== t) b.textContent = b.title = t };
  // One scroller: the frame fills the rest of the window, so the page around it never scrolls as well
  const fit = () => { if (!f.isConnected) return; f.style.height = Math.max(160, innerHeight - (f.getBoundingClientRect().top + scrollY) - parseFloat(getComputedStyle(document.body).paddingBottom || 0)) + 'px' };
  fit(); window.onresize = fit;
  // The frame's scrolling is read from here, every frame while it moves and ten times a second when it rests. The Mac app runs no script in
  // an article, and then no listener on it is ever called, its scroll events included. Once the text goes under the frame's top edge the
  // platform's separator shows there; the toolbar names the article; the reading place is kept for Back; a wide table fades at its edges.
  let lastY = -1, moved = 0;
  const watch = () => {
    if (!f.isConnected) return;
    const fw = f.contentWindow, t = f.lbTidy, now = performance.now();
    if (fw && fw.document.readyState === 'complete') {
      const y = fw.scrollY;
      if (y !== lastY) { lastY = y; moved = now; f.classList.toggle('scrolled', y > 2); scrolls[current] = y; barName() }
      if (t) for (const b of t.boxes) if (b.lbX !== b.scrollLeft) { b.lbX = b.scrollLeft; moved = now; t.edge(b) }
    }
    if (now - moved < 500) requestAnimationFrame(watch); else setTimeout(watch, 100);
  };
  watch();
  let shown = null;
  f.addEventListener('load', async () => {
    try {
      const fd = f.contentDocument;
      if (fd.contentType === 'application/json' && !(await away.here())) { f.hidden = true; return }   // the engine's answer, not an article: the library is out of reach, which the banner says
      lookOf(fd);
      if (!fd.getElementById('lb-article') && !fd.getElementById('lb-host')) {   // an older server adds no look of its own: the window's colour shows through, as crates/engine/src/library/saved_pages.rs does it
        const st = fd.createElement('style'); st.id = 'lb-host';
        st.textContent = 'html{--background-color-base:transparent!important}html,body,.mw-page-container,.mw-page-container-inner{background:transparent!important}.hatnote,.dablink,.rellink{--background-color-interactive-subtle:transparent!important;background:none!important}';
        (fd.head || fd.documentElement).appendChild(st);
      }
      f.lbTidy = null; try { f.lbTidy = tidyArticle(fd) } catch { }   // the article still reads as it came if this fails
      const fw = f.contentWindow;
      f.lbName = null;
      if (y && shown === null) restoreScroll(current, y, fw);   // Back to this article: its place, once (a link followed inside the frame starts at its top)
      const loc = f.contentWindow.location.pathname, docTitle = fd.title || '';
      const m = /^\/zim\/([^/]+)\/(.*)$/.exec(loc); if (!m) return;
      const p = decodeURIComponent(m[1]) + '/' + m[2].split('/').map(decodeURIComponent).join('/');
      const own = fd.querySelector('#firstHeading, h1'), raw = (own && own.textContent.trim()) || docTitle;
      if (!m[2] && !raw) return;   // the bare address, on its way to the front page
      const front = decodeURIComponent(loc) === await mainPage;   // the front page is named by its file, such as App/IntroPage: the encyclopedia's name says it instead
      if (f.contentDocument !== fd) return;
      if (front && own) own.style.setProperty('display', 'none', 'important');   // the skin sets its own display, which the hidden attribute does not beat
      main.classList.toggle('own-title', front);   // then the app's heading names it
      const nt = front ? zimLabel(zim) : raw || p.split('/').pop().replace(/_/g, ' '), sub = front ? t('app.row.encyclopedia') : zimLabel(zim);
      $('#wtitle').textContent = nt; if ($('#wopen')) $('#wopen').href = loc; setTitle(nt, '', zimLabel(zim));
      f.lbName = !front && own ? { nt, at: own.getBoundingClientRect().bottom + fw.scrollY } : null; barName();
      $('#wsave').innerHTML = saveButton({ key: 'w:' + p, title: nt, href: '#/wiki/' + p, sub });
      fit();
      if (p !== shown) { shown = p; history.replaceState(null, '', '#/wiki/' + p + (from ? '?from=' + encodeURIComponent(from) : '')); remember({ key: 'w:' + p, title: nt, href: '#/wiki/' + p, sub }) }
    } catch { }
  });
}
// The address an encyclopedia's bare address leads to (a redirect from the server, a refresh page in the Mac app), asked once
const ZMAIN = {};
const zimMain = zim => ZMAIN[zim] || (ZMAIN[zim] = fetch('/zim/' + encodeURIComponent(zim) + '/').then(async r => {
  let u = r.redirected ? r.url : '';
  if (!u) { const m = /http-equiv=["']?refresh["']?[^>]*url=([^"'>]+)/i.exec(await r.text()); if (m) u = new URL(m[1].replace(/&amp;/g, '&').trim(), r.url).href }
  return u ? decodeURIComponent(new URL(u).pathname) : '';
}).catch(() => ''));
// A table wider than the column scrolls sideways inside its own box (.lb-wide, styled by crates/engine/src/library/saved_pages.rs), so the article never does.
// The box goes around the table, which keeps its table layout: a table made a scrolling block itself turns its width into a fixed one.
// The box fades at an edge with more to scroll to. A list in a table cell is indented when its bullets would fall outside the cell's edge.
// Only with the server's article look, which carries the box's style; an older server's page stays as it came.
function tidyArticle(fd) {
  const fw = fd.defaultView, root = fd.querySelector('.mw-parser-output') || fd.body;
  if (!root || !fd.getElementById('lb-article') || fd.documentElement.dataset.lbTidy) return;
  fd.documentElement.dataset.lbTidy = '-';   // first, so the tables are measured as they will show (the look then undoes the skin's narrow-page blocks)
  const scrolls = e => { for (let a = e.parentElement; a && a !== root; a = a.parentElement) if (fw.getComputedStyle(a).overflowX !== 'visible') return true; return false };
  const boxes = [];
  // A box beside a float would shrink to the room left there (it scrolls, so nothing holds it open), where the table itself would go below
  // the float: the box is held at least as wide as the table can be made, up to the column. A float wider than the column would hang out on
  // the left, where a box cannot scroll to: in its box it is not floated.
  const wrap = (t, cs) => {
    const box = fd.createElement('div'); box.className = 'lb-wide';
    box.style.margin = `${cs.marginTop} 0 ${cs.marginBottom}`;   // the space above and below, which inside the box would no longer meet its neighbours'
    if (cs.clear !== 'none') box.style.clear = cs.clear;
    t.before(box); box.append(t);
    t.style.setProperty('margin-top', '0', 'important'); t.style.setProperty('margin-bottom', '0', 'important');
    if (cs.float !== 'none') { t.style.setProperty('float', 'none', 'important'); t.style.setProperty('margin-left', '0', 'important'); t.style.setProperty('margin-right', '0', 'important') }
    box.style.width = 'min-content'; const least = box.offsetWidth; box.style.width = '';
    box.style.minWidth = `min(100%, ${least}px)`;
    boxes.push(box); return box;
  };
  for (const t of root.querySelectorAll('table')) {
    if (t.parentElement.closest('table,.lb-wide') || scrolls(t)) continue;   // inside another table, or a box that scrolls already (a navbox)
    const cs = fw.getComputedStyle(t);
    if (cs.position === 'absolute' || cs.position === 'fixed' || (cs.float !== 'none' && t.offsetWidth <= root.clientWidth)) continue;   // a floated box that fits (an infobox) stays beside the text
    wrap(t, cs);
  }
  // Anything else still wider than the column (a bar chart, a box of fixed width, a group set inline) gets a box too, now and after a resize.
  const de = fd.documentElement;
  const rest = () => {
    if (de.scrollWidth <= de.clientWidth + 1) return;
    const cw = root.getBoundingClientRect(), out = e => { const r = e.getBoundingClientRect(); return r.width > 0 && (r.right > cw.right + 1 || r.left < cw.left - 1) };
    const found = [];
    for (const e of root.querySelectorAll('*')) {
      if (!out(e) || e.closest('.lb-wide') || found.some(f => f.contains(e))) continue;
      const p = e.parentElement; if (p !== root && out(p) && !scrolls(e)) continue;   // its parent is the one to box
      if (scrolls(e) || /^(TD|TH|TR|TBODY|THEAD|CAPTION|LI|SPAN|A|B|I|SUP|SUB)$/.test(e.tagName)) continue;
      const cs = fw.getComputedStyle(e); if (cs.position === 'absolute' || cs.position === 'fixed') continue;
      found.push(e);
    }
    found.forEach(e => { const b = wrap(e, fw.getComputedStyle(e)); edge(b); ro.observe(b) });
  };
  for (const li of root.querySelectorAll('table li')) {
    const s = fw.getComputedStyle(li), list = li.parentElement;
    if (s.display !== 'list-item' || s.listStyleType === 'none' || s.listStylePosition !== 'outside' || list.dataset.lbIn) continue;
    const r = li.getBoundingClientRect(), cell = li.closest('td,th'), c = cell ? cell.getBoundingClientRect() : r, m = parseFloat(s.fontSize) * 1.4;
    const rtl = s.direction === 'rtl', room = rtl ? c.right - r.right : r.left - c.left;
    if (room < m) { list.dataset.lbIn = 1; list.style.setProperty(rtl ? 'margin-right' : 'margin-left', `calc(${fw.getComputedStyle(list)[rtl ? 'marginRight' : 'marginLeft']} + ${Math.ceil(m - room)}px)`) }
  }
  const edge = b => {
    const more = b.scrollWidth - b.clientWidth, s = Math.abs(b.scrollLeft), rtl = fw.getComputedStyle(b).direction === 'rtl';
    b.classList.toggle(rtl ? 'lb-l' : 'lb-r', more > 1 && s < more - 1); b.classList.toggle(rtl ? 'lb-r' : 'lb-l', more > 1 && s > 1);
  };
  const ro = new fw.ResizeObserver(es => es.forEach(e => edge(e.target.closest('.lb-wide'))));
  boxes.forEach(b => { edge(b); ro.observe(b); ro.observe(b.firstElementChild) });   // as a box scrolls, wiki() calls edge (it reads the scroll, see there)
  // What is left of the web page's machinery, which reads as a broken page offline: editing links where the link itself was taken out
  // ('(edit it)', '[edit on Wikidata]'), a template's styles error, a chart's source where the chart would be, an empty paragraph
  // holding only a line break (a gap under the title), and a note's mark written out as its counter ('[lower-alpha 1]' for '[a]').
  const tw = fd.createTreeWalker(root, NodeFilter.SHOW_TEXT, { acceptNode: n => /edit|<graph|content model|-alpha|-roman/i.test(n.data) ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_SKIP });
  const gone = [], ab = n => { let s = ''; for (n++; n > 0; n = Math.floor((n - 1) / 26)) s = String.fromCharCode(97 + (n - 1) % 26) + s; return s };
  for (let n; (n = tw.nextNode());) {
    const e = n.parentElement; if (!e || e === root) continue;
    const t = e.textContent.trim();
    if (/^[[(]\s*edit(\s+it|\s+on\s+wikidata)?\s*[\])]$/i.test(t) || (e.matches('.error') && /must have content model/.test(t))) gone.push(e);
    else if (/^<graph>[\s\S]*<\/graph>$/.test(t)) gone.push(e);
    else if (/<graph>[\s\S]*?<\/graph>/.test(n.data)) n.data = n.data.replace(/<graph>[\s\S]*?<\/graph>/g, '');
    else if (e.closest('sup.reference')) { const m = /^(lower|upper)-(alpha|roman) (\d+)$/.exec(n.data.trim()); if (m) { const k = +m[3], s = m[2] === 'alpha' ? ab(k - 1) : roman(k); n.data = m[1] === 'upper' ? s.toUpperCase() : s } }
  }
  for (const p of root.querySelectorAll('p')) if (!p.textContent.trim() && !p.querySelector('img,svg,video,audio,iframe,input,math,object')) gone.push(p);
  gone.forEach(e => e.style.setProperty('display', 'none', 'important'));
  rest(); let rt = 0; new fw.ResizeObserver(() => { clearTimeout(rt); rt = setTimeout(rest, 200) }).observe(root);
  fd.documentElement.dataset.lbTidy = boxes.length;
  return { boxes, edge };   // boxes: also those rest() adds later
}
const roman = n => { let s = ''; for (const [v, r] of [[1000, 'm'], [900, 'cm'], [500, 'd'], [400, 'cd'], [100, 'c'], [90, 'xc'], [50, 'l'], [40, 'xl'], [10, 'x'], [9, 'ix'], [5, 'v'], [4, 'iv'], [1, 'i']]) for (; n >= v; n -= v) s += r; return s };
// The article's stylesheet (crates/engine/src/library/saved_pages.rs) reads these: the window's text colours and its accent, which only this page knows
// (the system accent in the apps, Lastbook green in a browser).
function lookOf(fd) {
  const cs = getComputedStyle(html), r = fd.documentElement.style;
  const probe = document.createElement('i'); probe.style.color = 'var(--accent)'; probe.hidden = true; document.body.append(probe);
  r.setProperty('--lb-link', getComputedStyle(probe).color); probe.remove();
  for (const [k, v] of [['--lb-ink', '--ink'], ['--lb-ink2', '--ink2'], ['--lb-line', '--line2'], ['--lb-font', '--font'], ['--lb-display', '--display']]) { const x = cs.getPropertyValue(v).trim(); if (x) r.setProperty(k, x) }
}
const ZIMS = [['mdwiki_en_all_maxi_2025-11.zim', 'app.zim.mdwiki', 'app.zim.mdwiki_desc', 'health'],   // file, name, description (keys), icon
['wikibooks_en_all_maxi_2026-04.zim', 'app.zim.wikibooks', 'app.zim.wikibooks_desc', 'book'],
['appropedia_en_all_maxi_2026-02.zim', 'app.zim.appropedia', 'app.zim.appropedia_desc', 'repair'],
['wikipedia_en_all_nopic_2026-06.zim', 'app.zim.wikipedia', 'app.zim.wikipedia_desc', 'wiki']];
const zimsHere = async () => { const here = (await libInfo()).zims; return here ? ZIMS.filter(z => here.includes(z[0])) : ZIMS };
async function encyclopedias(q) {
  setTitle(t('app.zim.title'));
  const zims = await zimsHere();
  if (!zims.length) { main.innerHTML = `<h1>${th('app.zim.title')}</h1><p class="lead">${th('app.zim.none')}</p>`; return }
  main.innerHTML = `<h1>${th('app.zim.title')}</h1><p class="lead">${th('app.zim.lead', { n: zims.length })}</p>
  <div style="margin:16px 0">${searchBox('zq', q, t('app.zim.ph'))}</div><div id="zres"></div>
  ${secHead(th('app.zim.open_one'))}<div class="cards2">${zims.map(z => `<a class="card" href="#/wiki/${z[0]}/?from=${encodeURIComponent('#/encyclopedias')}"><span class="ico">${ico(z[3])}</span><b>${th(z[1])}</b><small>${th(z[2])}</small></a>`).join('')}</div>`;
  $('#zqF').addEventListener('submit', e => { e.preventDefault(); location.hash = '#/encyclopedias?q=' + encodeURIComponent($('#zq').value.trim()) });
  if (q) zimResults(q, $('#zres'), 40);
}
// heading: part of a wider search, shown under that heading only when something matched. Answers whether anything did.
async function zimResults(q, el, max = 8, heading = '') {
  el.innerHTML = heading ? '' : `<p class="loading">${th('app.zim.searching')}</p>`;
  try {
    const hits = await api('/api/collection-search?q=' + encodeURIComponent(q));
    if (!hits.length) { el.innerHTML = heading ? '' : `<div class="empty"><span class="e-ico">${ico('wiki')}</span><b>${th('app.zim.no_articles')}</b><p>${th('app.zim.try_another')}</p></div>`; return false }
    const ord = ['mdwiki', 'wikibooks', 'appropedia', 'wikipedia'];
    hits.sort((a, b) => ord.findIndex(k => a.collection.startsWith(k)) - ord.findIndex(k => b.collection.startsWith(k)));
    el.innerHTML = `${heading ? `<h2>${heading}</h2>` : ''}<ul class="list">${hits.slice(0, max).map(h => `<li><a class="row" href="#/wiki/${esc(h.path.replace(/^zim\//, ''))}?${fromQ()}"><span class="k">${ico('wiki')}</span><span><b>${esc(h.title)}</b><small>${esc(zimLabel(h.collection))}</small></span></a></li>`).join('')}</ul>`;
    return true;
  } catch (e) { el.innerHTML = heading || (e && e.away) ? '' : `<p class="muted">${th('app.zim.unavailable', { error: e.message })}</p>`; return false }   // the library out of reach: the banner says so
}

/* ------------------------------------------------------------- search -- */
const QSTOP = new Set('a an and are can do does for from get got have how i if in is it its me my need no not of on or our so that the this to was we what when where which who why will with without you your won don can t s'.split(' '));
// Words are split, folded and matched by web/words.js (loaded first), so accents, Chinese and other scripts work.
function topicMatches(q) {
  return typeof SearchWords === 'undefined' ? [] : SearchWords.rankTopics(q, TOPICS, QSTOP);
}
let searchSeq = 0;
async function searchPage(q, kind) {
  setTitle(q ? t('app.search.title_q', { q }) : t('app.search.title'));
  const kinds = [['all', 'app.search.kind_all'], ['read', 'app.search.kind_read'], ['video', 'nav.videos'], ['wiki', 'app.zim.title']];
  main.innerHTML = `<h1>${q ? th('app.search.results') : th('app.search.title')}</h1>
  ${searchBox('searchQ', q, t('app.search.ph'))}
  <div class="chips" role="group" aria-label="${th('app.search.show')}" style="margin-top:12px" id="kinds">${kinds.map(([k, l]) => `<a class="chip ${k === kind ? 'on' : ''}" aria-pressed="${k === kind}" data-kind="${k}" href="#/search?q=${encodeURIComponent(q)}&kind=${k}">${th(l)}</a>`).join('')}</div>
  <div id="sask"></div><div id="sout"></div>`;
  const input = $('#searchQ');
  $('#searchQF').addEventListener('submit', e => { e.preventDefault(); input.blur(); go(input.value.trim(), true) });
  let tmr;
  input.addEventListener('input', () => { clearTimeout(tmr); tmr = setTimeout(() => go(input.value.trim(), false), 350) });
  // as-you-type: update results and the address; a new search is one new history entry, not one per keystroke
  let fresh = true;   // the next change starts a new entry
  function go(nq, commit) {
    const h = '#/search?q=' + encodeURIComponent(nq) + (kind !== 'all' ? '&kind=' + kind : '');
    if (nq === q) { if (commit) fresh = true; return }
    if (fresh) pushHere(h); else history.replaceState(null, '', h);
    fresh = !!commit; q = nq; if (input.value !== nq) input.value = nq;
    $$('#kinds a').forEach(a => a.href = `#/search?q=${encodeURIComponent(nq)}&kind=${a.dataset.kind}`);
    askOffer(nq); results(nq, kind);
  }
  searchLive = (nq, commit, typing) => { clearTimeout(tmr); if (typing) tmr = setTimeout(() => go(nq, false), 350); else go(nq, commit) };
  setTimeout(() => { $('#topQ').value = q }, 0);   // after the router clears it: on Windows the pane's field is this page's field
  askOffer(q); results(q, kind);
  if (!q) setTimeout(() => findBox().focus(), 50);
}
function askOffer(q) {
  const el = $('#sask'); if (!el) return;
  el.innerHTML = q && q.split(/\s+/).length >= 2 ? `<a class="ghit" href="#/ask?q=${encodeURIComponent(q)}" style="--tc:var(--accent)"><span class="ico">${ico('ask')}</span><span><b>${th('app.search.ask', { q })}</b><small>${th('app.search.ask_sub')}</small></span></a>` : '';
}
// Example searches on an empty search page, in the interface's language
const SEARCH_EXAMPLES = ['app.search.ex_water', 'app.search.ex_leg', 'app.search.ex_co', 'app.search.ex_cold', 'app.search.ex_radio', 'app.search.ex_potatoes', 'app.search.ex_knife', 'app.search.ex_diarrhoea'];
async function results(q, kind) {
  const seq = ++searchSeq, out = $('#sout');
  if (!q) {
    out.innerHTML = `<p class="muted" style="margin-top:18px">${th('app.search.hint')}</p><div class="chips wrapchips">${SEARCH_EXAMPLES.map(k => t(k)).map(x => `<a class="chip" href="#/search?q=${encodeURIComponent(x)}">${esc(x)}</a>`).join('')}</div>`;
    return;
  }
  const g = kind === 'all' ? topicMatches(q) : [];
  out.innerHTML = `${g.length ? `<h2>${th('app.search.guides')}</h2>${g.map(({ t: topic, c }) => `<a class="ghit" href="#/t/${topic.id}${c ? '/' + c.id : ''}" ${tcStyle(topic.id)}><span class="ico">${ico(topic.id)}</span><span><b>${esc(shortOf(topic.id, topic.title))}${c ? ' — ' + esc(c.label) : ''}</b><small>${th('app.search.guide_sub')}</small></span></a>`).join('')}` : ''}
    <div id="spass"></div><div id="svid"></div><div id="sres"></div><div id="szim"></div>`;
  if (kind === 'wiki') return zimResults(q, $('#szim'), 40);
  // Each part answers whether it found anything; "nothing matched" shows once, only when none did.
  const parts = kind === 'all' || kind === 'read' ? [passages(q, seq)] : [];
  const reqs = [api('/api/find?q=' + encodeURIComponent(q) + '&kind=' + (kind === 'video' ? 'video' : 'read') + '&lang=' + I18N.lang)];
  if (kind === 'all') reqs.push(api('/api/find?q=' + encodeURIComponent(q) + '&kind=video&lang=' + I18N.lang).catch(() => null));
  $('#sres').innerHTML = `<p class="loading">${th('app.search.searching')}</p>`;
  const found = (async () => {
    try {
      const [d, vd] = await Promise.all(reqs);
      if (seq !== searchSeq) return true;
      const vids = (kind === 'video' ? d : vd)?.results || [];
      if (kind === 'video') { $('#sres').innerHTML = vids.length ? `<div class="vgrid" style="margin-top:14px">${vids.map(r => vcard(r)).join('')}</div>` : ''; return vids.length > 0 }
      if (vids.length) { $('#svid').innerHTML = secHead(th('nav.videos'), '', `<a href="#/search?q=${encodeURIComponent(q)}&kind=video">${th('app.search.see_all')}</a>`) + rail(vids.slice(0, 10).map(r => vcard(r)).join('')); wireRails($('#svid')) }
      $('#sres').innerHTML = d.results.length ? `<h2>${th('app.search.in_manuals')}</h2>
        ${d.expanded.length ? `<p class="small muted">${th('app.search.also', { words: d.expanded.slice(0, 10).join(', ') })}${d.mode === 'some words' ? ' · ' + th('app.search.some_words') : ''}</p>` : ''}
        <div class="results">${d.results.map(resultCard).join('')}</div>` : '';
      return d.results.length + vids.length > 0;
    } catch (e) { if (seq === searchSeq) $('#sres').innerHTML = e && e.away ? '' : errorBox(e, t('app.search.no_response')) + `<p class="small muted">${th('app.search.busy')}</p>`; return true }   // the library out of reach: the banner says so
  })();
  parts.push(found);
  if (kind === 'all') parts.push(found.then(() => seq === searchSeq && zimResults(q, $('#szim'), 6, th('app.search.in_zims'))));
  const any = (await Promise.all(parts)).some(Boolean);
  if (seq === searchSeq && !g.length && !any) $('#sres').innerHTML = emptySearch(q);
}
// Most relevant pages from the shared keyword + meaning-based retriever (the same one the assistant uses).
async function passages(q, seq) {
  try {
    const d = await api('/api/retrieve?q=' + encodeURIComponent(q) + '&k=6&kind=manual&lang=' + I18N.lang);
    if (seq !== searchSeq || !d.results.length || !$('#spass')) return false;
    $('#spass').innerHTML = secHead(th('app.search.relevant'), d.semantic && !d.semantic.chunks ? th('app.search.by_words') : th('app.search.by_meaning')) + `<div class="rcards">${d.results.map(r => rcard({ src: { id: r.doc_id, kind: r.kind, title: r.title, publisher: r.publisher, year: r.year, pages: r.pages, trust: r.trust, trust_code: r.trust_code }, page: r.page, label: r.title, note: (r.excerpt || '').slice(0, 220) + '…' })).join('')}</div>`;
    return true;
  } catch { return false }
}
const emptySearch = q => `<div class="empty"><span class="e-ico">${ico('search')}</span><b>${th('app.search.none', { q })}</b><p>${th('app.search.none_hint')}</p></div>`;
function resultCard(r) {
  const base = { src: r };
  return `<article class="result"><h3><a href="${hrefFor({ ...base, page: r.hits[0] && r.hits[0].page })}">${esc(r.title)}</a></h3>
    <div class="rsrc">${srcText(r)} · ${esc(trustOf(r))}</div>
    ${r.hits.slice(0, 2).map(h => `<a class="hit" href="${hrefFor({ ...base, page: h.page })}">${h.page ? `<b>${th('app.search.page', { page: h.page })}</b> ` : ''}${h.snippet}</a>`).join('')}</article>`;
}

/* ------------------------------------------------------------ library -- */
async function library() {
  setTitle(t('app.lib.title'));
  const saved = store.get('saved', []).length, chats = (typeof Chat !== 'undefined' ? Chat.count() : 0);
  const zims = await zimsHere();
  main.innerHTML = `<h1>${th('app.lib.title')}</h1><p class="lead">${th('app.lib.lead')}</p>
  <div class="cards2 three" style="margin-top:16px">
    <a class="card" href="#/saved"><span class="ico">${ico('save')}</span><b>${th('app.saved.title')}</b><small>${saved ? th('app.lib.saved_n', { n: saved }) : th('app.lib.saved_none')}</small></a>
    <a class="card" href="#/ask"><span class="ico">${ico('ask')}</span><b>${th('app.lib.chats')}</b><small>${chats ? th('app.lib.chats_n', { n: chats }) : th('app.lib.chats_none')}</small></a>
    <a class="card" href="#/prepare"><span class="ico">${ico('prep')}</span><b>${th('app.prep.title')}</b><small>${th('app.lib.prep_sub')}</small></a>
  </div>
  ${secHead(th('app.learn.title'), th('app.lib.learn_sub'))}
  <div class="needs">${learnTopics().map(topic => `<a class="need" href="#/t/${topic.id}" ${tcStyle(topic.id)}><span class="ico">${ico(topic.id)}</span><b>${esc(shortOf(topic.id, topic.title))}</b><small>${esc(topic.short || '')} · ${th('app.learn.steps', { n: (topic.route || []).length })}</small></a>`).join('')}</div>
  ${zims.length ? secHead(th('app.zim.title'), th('app.lib.zims_sub'), `<a href="#/encyclopedias">${th('app.lib.zims_search')}</a>`) + `<div class="cards2">${zims.map(z => `<a class="card" href="#/wiki/${z[0]}/?from=${encodeURIComponent('#/library')}"><span class="ico">${ico(z[3])}</span><b>${th(z[1])}</b><small>${th(z[2])}</small></a>`).join('')}</div>` : ''}
  ${secHead(th('app.lib.browse'), th('app.lib.browse_sub'))}<div id="shelves"><p class="loading">${th('app.loading')}</p></div>
  <p style="margin-top:20px"><a href="#/about">${th('app.lib.about_link')}</a>${IN_WINDOW ? ` · <a href="/ui/setup.html#manage">${th('app.lib.setup_link')}</a>` : ''}</p>`;
  try {
    const sh = await api('/api/shelves');
    $('#shelves').innerHTML = sh.filter(s => s.read + s.video).map(s => `<a class="shelfrow" href="#/shelf/${s.id}"><span><b>${esc(shelfName(s))}</b><small>${s.read ? th('app.lib.to_read', { count: s.read }) : ''}${s.read && s.video ? ' · ' : ''}${s.video ? th('app.lib.videos_n', { n: s.video }) : ''}</small></span>${ico('right')}</a>`).join('');
  } catch (e) { $('#shelves').innerHTML = errorBox(e) }
}
async function prepareHub() {
  setTitle(t('app.prep.title'));
  const list = nowTopics().filter(topic => (topic.prepare || []).length);
  main.innerHTML = `<h1>${th('app.prep.title')}</h1>
  <p class="lead">${th('app.prep.lead')}</p>
  <div class="notice">${th('app.prep.notice')}</div>
  ${list.map(topic => `<details class="more" ${tcStyle(topic.id)}><summary><span class="prep-ico">${ico(topic.id)}</span><span>${esc(shortOf(topic.id, topic.title))} <span class="n">· ${topic.prepare.length}</span></span></summary><div class="body"><ul class="list">${topic.prepare.map(i => rowItem({ ...i, preview: true })).join('')}</ul><p><a href="#/t/${topic.id}">${th('app.prep.open_guide')}</a></p></div></details>`).join('')}`;
}
async function shelfPage(id, kind, q) {
  const sh = (await api('/api/shelves')).find(s => s.id === id);
  if (!sh) return notFound();
  const title = shelfName(sh);
  setTitle(title);
  main.innerHTML = `${backButton('#/library', t('app.lib.title'))}${crumbs([[t('app.lib.title'), '#/library'], [title]])}<h1>${esc(title)}</h1>
  <div style="margin:14px 0 10px">${searchBox('shq', q, t('app.shelf.ph'), t('app.shelf.filter'))}</div>
  <div class="chips">${[['all', 'app.search.kind_all'], ['read', 'app.shelf.read'], ['video', 'nav.videos']].map(([k, l]) => `<a class="chip ${k === kind ? 'on' : ''}" href="#/shelf/${id}?kind=${k}&q=${encodeURIComponent(q)}">${th(l)}</a>`).join('')}</div><div id="shres"><p class="loading">${th('app.loading')}</p></div>`;
  $('#shqF').addEventListener('submit', e => { e.preventDefault(); location.hash = `#/shelf/${id}?kind=${kind}&q=` + encodeURIComponent($('#shq').value.trim()) });
  let off = 0;
  const load = async () => {
    const d = await api(`/api/shelf/${id}?kind=${kind}&q=${encodeURIComponent(q)}&offset=${off}`);
    const html = d.collections.map(c => `<section class="collection"><h3>${esc(collName(c))}</h3><div class="small muted">${esc(trustOf(c))} · ${th('app.shelf.items', { n: c.count })}</div><ul class="list">${c.items.map(r => rowItem({ src: r })).join('')}</ul>${c.count > c.items.length ? `<a class="btn ghost" href="#/collection?name=${encodeURIComponent(c.collection)}&${fromQ()}">${th('app.see_all', { count: c.count })}</a>` : ''}</section>`).join('');
    if (!off) $('#shres').innerHTML = d.collections.length ? '' : `<div class="empty">${th('app.shelf.none')}</div>`;
    $('#shres').insertAdjacentHTML('beforeend', html);
    $('#moreb')?.remove();
    if (d.more) { $('#shres').insertAdjacentHTML('beforeend', `<p><button class="btn plain" id="moreb">${th('app.more')}</button></p>`); $('#moreb').onclick = () => { off += 40; load() } }
  };
  await load();
}
async function collectionPage(name, from) {
  setTitle(name);
  main.innerHTML = `<div>${backButton(from, t('app.lib.title'))}</div><h1 style="margin-top:12px">${esc(name)}</h1><ul class="list collection" id="cl"></ul><div id="cm"></div>`;
  let off = 0;
  const load = async () => {
    const d = await api('/api/collection?name=' + encodeURIComponent(name) + '&offset=' + off);
    $('#cl').insertAdjacentHTML('beforeend', d.items.map(r => rowItem({ src: r })).join(''));
    $('#cm').innerHTML = d.more ? `<button class="btn plain" id="cmb">${th('app.more')}</button>` : '';
    if (d.more) $('#cmb').onclick = () => { off += 60; load() };
  };
  await load();
}

/* -------------------------------------------------------------- saved -- */
async function savedPage() {
  setTitle(t('app.saved.title'));
  const s = store.get('saved', []), r = await recents();
  const li = x => `<li><a class="row" href="${esc(x.href)}"><span class="k">${ico(x.key[0] === 'v' ? 'video' : x.key[0] === 'w' ? 'wiki' : x.key[0] === 't' ? 'book' : x.key[0] === 'a' ? 'ask' : 'page')}</span><span><b>${esc(x.title)}</b><small>${esc(rowSub(x))}</small></span></a></li>`;
  const cont = await resumable();
  main.innerHTML = `<h1>${th('app.saved.title')}</h1><p class="lead">${th('app.saved.lead')}</p>
  ${s.length ? `<div class="collection" style="margin-top:18px"><ul class="list">${s.map(li).join('')}</ul></div>` : `<div class="empty"><span class="e-ico">${ico('save')}</span><b>${th('app.saved.none')}</b><p>${th('app.saved.none_hint')}</p></div>`}
  ${cont.length ? secHead(th('app.videos.continue')) + rail(cont.map(([id, v]) => vcard({ id, title: v.title, thumb: v.thumb, duration: v.d }, { sub: t('app.video.resume_at', { time: clock(v.t) }) })).join('')) : ''}
  ${secHead(th('app.saved.recent'))}${r.length ? `<div class="collection"><ul class="list">${r.map(li).join('')}</ul></div><p><button class="btn plain" id="clr">${th('app.saved.clear')}</button></p>` : `<div class="empty"><p>${th('app.saved.recent_none')}</p></div>`}`;
  const c = $('#clr'); if (c) c.onclick = () => { store.set('recent', []); savedPage() };
}

/* -------------------------------------------------------------- about -- */
// What the assistant cannot read, as a sentence: counts of none are left out, and one is one
function blind(c) {
  if (!c) return '';
  const nv = c.videos - c.videos_with_text, parts = [];
  if (nv) parts.push(t('app.about.blind_videos', { n: nv, total: c.videos }));
  if (c.pdfs_image_only) parts.push(t('app.about.blind_pdfs', { n: c.pdfs_image_only }));
  if (c.docs_without_text) parts.push(t('app.about.blind_docs', { n: c.docs_without_text }));
  if (!parts.length) return '';
  const all = I18N.list(parts);
  return esc(all[0].toUpperCase() + all.slice(1) + '. ');
}
// The optional local AI in the desktop app: what it is, where it runs, and its controls; the models found here, when there are
// several. '' in the server version. What the engine says comes as codes with params, worded by web/chat.js (Chat.said and
// the others), as on the chat page.
function localAbout(st) {
  const l = st.local; if (!l) return '';
  // placement: where each part runs ([{device, bytes}]); the runner of this engine names only how ("fit", "disk_cpu"), not shown
  const where = I18N.list((Array.isArray(l.placement) ? l.placement : []).filter(p => p.bytes > 2e8).map(p => t('app.ai.on_device', { size: I18N.unit(p.bytes / 1e9, 'gigabyte', 1), device: p.device })));
  const line = { ready: 'app.ai.running', stopped: 'app.ai.stopped', loading: 'app.ai.starting' }[l.state];
  const btn = (a, label) => `<button class="btn plain" onclick="localAI(${esc(JSON.stringify(a))})">${label}</button>`;
  const row = (...b) => `<span class="row" style="flex-wrap:wrap">${b.join('')}</span>`;
  const disk = l.verdict === 'from_disk' && l.from_disk_chosen, models = Chat.models(l, btn);
  if (line) return `${th('app.ai.line', { name: l.name, state: t(line) })}${where ? ' ' + th('app.ai.uses', { where }) : ''} ${disk ? th('app.ai.disk') + ' ' : ''}${th('app.ai.text_only')}
      ${row(l.state === 'ready' ? btn('stop', th('app.ai.stop')) : '', btn('off', th('app.ai.off')), disk ? btn('from_disk_off', th('chat.setup.disk_off')) : '')}${models}`;
  if (l.state === 'off') return `${th('app.ai.is_off')} ${btn('on', th('app.ai.on'))}`;
  if (l.state === 'from_disk') return `${esc(Chat.why(l))} ${Chat.diskWarning(l)}
      ${row(btn('from_disk_on', th('chat.setup.disk_on')), btn('rescan', th('app.ai.rescan')))}${models}`;
  const status = l.state === 'absent' ? t('app.ai.absent', { size: I18N.unit(17, 'gigabyte') }) : l.state === 'measuring' ? t('app.ai.measuring')
    : l.state === 'checking' ? (l.pct != null ? t('app.ai.checking_pct', { pct: l.pct }) : t('app.ai.checking')) : Chat.localLine(l)[2];
  return `${(['absent', 'measuring', 'checking'].includes(l.state) ? th('app.ai.status', { status }) : esc(status))} ${l.state === 'absent' ? `<a class="btn plain" href="/ui/setup.html#manage">${th('app.ai.add')}</a>` : ''}${btn('rescan', th('app.ai.rescan'))}${models}`;
}
async function localAI(action) {
  let a = null;
  try { a = await (await fetch('/api/assistant/local', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ action }) })).json() } catch { }
  const no = Chat.refusal(a); if (no) toast(no);   // the button could not act (no model found, no library open): said by its code
  setTimeout(() => { if (location.hash.startsWith('#/about')) route() }, 400);
}
// What the library holds, as a sentence: each count its own plural phrase (1 saved web page, 2 saved web pages), the manuals with
// their pages in brackets, joined as the language joins a list
function contents(s) {
  const n = v => typeof v === 'number' && isFinite(v) ? v : 0;
  const pdfs = t('app.about.with_pages', { what: t('app.about.n_pdfs', { n: n(s.pdfs) }), pages: t('app.about.n_pages', { n: n(s.pdf_pages) }) });
  return t('app.about.contents_list', { list: I18N.list([pdfs, t('app.about.n_web', { n: n(s.web_pages) }), t('app.about.n_videos', { n: n(s.videos) }), t('app.about.n_zims', { n: n(s.collections) })]) });
}
// How sources are labelled: the label, and who it covers
const LABELS = [['app.about.l_intl', 'app.about.l_intl_who'], ['app.about.l_hesperian', 'app.about.l_hesperian_who'], ['app.about.l_gov', 'app.about.l_gov_who'],
  ['app.about.l_uni', 'app.about.l_uni_who'], ['app.about.l_maker', 'app.about.l_maker_who']];
async function about() {
  setTitle(t('app.about.title'));
  let dl = {}, st = {};
  LIB = null;   // About shows the counts as they are now
  const s = await libInfo();
  const oldServer = s.zims === undefined;   // it still serves the reports; this code does not
  try { dl = await api('/api/download-status') } catch { }
  try { st = await fetch('/api/assistant/status').then(r => r.json()) } catch { }   // not api(): this answer names an error when no model is running, and still holds what is wanted
  const ch = dl.channels && dl.channels.counts, ix = st.index || {};
  main.innerHTML = `<h1>${th('app.about.title')}</h1>
  <p class="lead">${th('app.about.lead')}</p>
  <div class="cards2" style="margin-top:16px">
   <div class="card"><b>${th('app.about.contents')}</b><small>${esc(contents(s))}</small></div>
   ${ch ? `<div class="card"><b>${th('app.about.arriving')}</b><small>${th('app.about.arriving_text', { pending: ch.pending, downloading: ch.downloading })}</small></div>` : ''}
   <div class="card"><b>${th('app.about.assistant')}</b><small>${localAbout(st) || (st.online ? th('app.about.model_ok', { model: st.model }) : th('app.about.no_model'))} ${ix.chunks ? th(ix.pending ? 'app.about.index_pending' : 'app.about.index', { chunks: ix.chunks, docs: ix.docs, pending: ix.pending }) : ''}</small></div>
   <div class="card"><b>${th('app.about.blind_title')}</b><small>${blind(st.coverage)}${st.vision === false ? th('app.about.blind_text') : th('app.about.blind_text_vision')}${st.coverage && st.coverage.remote_videos ? ' ' + th('app.about.remote', { n: st.coverage.remote_videos }) : ''}</small></div>
   ${IN_WINDOW && window.LBUpdate ? LBUpdate.card() : ''}
  </div>
  <h2>${th('app.about.labels')}</h2><ul>
   ${LABELS.map(([label, who]) => `<li><b>${th(label)}</b> — ${th(who)}</li>`).join('\n   ')}
   <li>${th('app.about.l_wiki_line', { wiki: I18N.raw(`<b>${th('app.about.l_wiki')}</b>`), video: I18N.raw(`<b>${th('app.about.l_hobby')}</b>`) })}</li></ul>
  <h2>${th('app.about.not_do')}</h2><ul>
   <li>${th('app.about.nd_pages')}</li>
   <li>${th('app.about.nd_help')}</li>
   <li>${th('app.about.nd_practice')}</li>
   ${s.pack ? '' : `<li>${th('app.about.nd_lists')}</li>`}</ul>
  ${s.sources ? `<h2>${th('app.about.sources')}</h2><p>${th('app.about.sources_text', { link: I18N.raw(`<a href="#/read/${encodeURIComponent(s.sources)}?${fromQ()}">${th('app.about.sources_link')}</a>`) })}</p>` : ''}
  <h2>${th('app.about.origin')}</h2><p>${th('app.about.origin_text')}</p>
  ${s.contact ? `<h2>${th('app.about.contact')}</h2><p>${th('app.about.contact_text', { email: s.contact })}</p>` : ''}
  ${oldServer ? `<h2>${th('app.about.other')}</h2><p><a href="/reports/gap-closure-status.html" target="_blank">${th('app.about.r_gap')}</a> · <a href="/reports/learning-pathways.html" target="_blank">${th('app.about.r_paths')}</a> · <a href="/reports/survival-tools-cart.html" target="_blank">${th('app.about.r_supplies')}</a></p>` : ''}`;
  if (IN_WINDOW && window.LBUpdate) LBUpdate.mount(main);   // Check for updates (web/update.js): only when the person asks
}

/* the language menu in the sidebar (web/i18n.js). A new language redraws the page in place, from its address, which keeps
   where the person is; the map is stopped first, as leaving it would. */
I18N.picker($('#langSel'), $('#langName'));
I18N.picker($('#langSelTop'));   // the toolbar's globe, where the sidebar is not (narrower than 640)
addEventListener('lastbook:lang', () => { if (typeof MapView !== 'undefined') MapView.stop(); route(); topPlaceholder(); railTitles() });
I18N.ready.then(route);
if ('serviceWorker' in navigator && /^(127\.0\.0\.1|localhost)$/.test(location.hostname)) navigator.serviceWorker.register('/sw.js').catch(() => { });
