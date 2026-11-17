'use strict';
/* Lastbook setup: choose a library and the local AI, then download, check, unpack and switch. The contract is docs/SETUP-API.md.
   Works on its own page in setup mode (no library, so none of /api/topics and the rest) and inside the app (Library → Library and local AI).
   Views: manage (what is here now), choose (what to get), job (progress, pause, errors), done. Plain fetch: in the app the bridge adds the key.
   Words: t() and th() from web/i18n.js, keys setup.* in web/locales/. What the engine says comes as a code with params beside its English
   sentence (CODE, REC_WHY, AI_WHY; a job's error in errorBox), said here in the person's language; an unknown code shows the engine's English.
   A library can live on this computer's own disk, an external disk or USB stick, or a network share (a NAS): each disk shows its kind. */
(() => {
  const $ = s => document.querySelector(s);
  const main = $('#main'), live = $('#live');
  const esc = s => String(s == null ? '' : s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const WIN = document.documentElement.classList.contains('plat-win');
  const UNIT = WIN ? 1024 : 1000;   // each system's own way of counting a gigabyte, so the numbers match Finder or Explorer
  const CAN = { removeAi: true, repair: true };   // one switch each, should a build of the engine be without remove-ai or repair
  const MANAGE_ASKED = /manage/.test(location.hash);   // #manage: the bridge allows no ?query on the top page

  const ICO = {
    disk: '<rect x="3" y="5" width="18" height="14" rx="3"/><path d="M7 15h.01M11 15h6"/>',
    chip: '<rect x="6" y="6" width="12" height="12" rx="2"/><path d="M9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4"/>',
    gpu: '<rect x="2" y="6" width="20" height="12" rx="2"/><circle cx="9" cy="12" r="3"/><path d="M15 10h3M15 14h3"/>',
    book: '<path d="M4 4v16M9 4v16M14 5l4 15M3 20h18"/>',
    ai: '<path d="M4 5h16v11H9l-5 4z"/><path d="M9 9.5h6M9 12.5h4"/>',
    ok: '<circle cx="12" cy="12" r="9"/><path d="m8 12 3 3 5-6"/>',
    folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
    warn: '<path d="M12 3 2 20h20z"/><path d="M12 10v4M12 17h.01"/>',
    file: '<path d="M6 3h8l4 4v14H6z"/><path d="M14 3v4h4"/>',
    pause: '<path d="M9 6v12M15 6v12"/>',
    play: '<path d="m8 5 11 7-11 7z"/>',
    back: '<path d="M15 18 9 12l6-6"/>',
    x: '<path d="M6 6l12 12M18 6 6 18"/>',
    retry: '<path d="M20 11a8 8 0 1 0-2.3 5.7"/><path d="M20 4v7h-7"/>',
    repair: '<path d="M14.7 6.3a4 4 0 0 0-5.4 5.2L3 17.8 6.2 21l6.3-6.3a4 4 0 0 0 5.2-5.4l-2.6 2.6-2.4-.6-.6-2.4z"/>',
    down: '<path d="M12 5v14M5 12l7 7 7-7"/>',
    up: '<path d="M12 19V5M5 12l7-7 7 7"/>',
    map: '<path d="M9 4 3 6v14l6-2 6 2 6-2V4l-6 2z"/><path d="M9 4v14M15 6v14"/>',
    trash: '<path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/>',
    usb: '<path d="M9 2h6v5H9z"/><rect x="7" y="7" width="10" height="15" rx="2"/><path d="M11 4.5h.01M13 4.5h.01"/>',
    nas: '<rect x="4" y="3" width="16" height="7" rx="2"/><rect x="4" y="14" width="16" height="7" rx="2"/><path d="M8 6.5h.01M8 17.5h.01M12 10v4"/>',
  };
  const ico = (k, cls) => `<svg viewBox="0 0 24 24" aria-hidden="true"${cls ? ` class="${cls}"` : ''}>${ICO[k] || ''}</svg>`;

  /* ------------------------------------------------------------ numbers */
  const size = b => I18N.size(b, UNIT);
  const ram = b => I18N.unit(Math.round(b / 1024 ** 3), 'gigabyte');   // memory is counted in binary by both systems
  const rate = bps => { if (!(bps > 0)) return ''; const m = bps / UNIT ** 2; return m >= 10 ? I18N.unit(Math.round(m), 'megabyte-per-second') : I18N.unit(m, 'megabyte-per-second', 1) };
  function eta(s) {
    if (s == null || !isFinite(s) || s < 0) return '';
    if (s < 60) return t('setup.eta.under_minute');
    const m = Math.round(s / 60);
    if (m < 60) return t('setup.eta.minutes', { n: m });
    const h = Math.floor(m / 60), r = m % 60;
    return r ? t('setup.eta.hours_minutes', { h, m: r }) : t('setup.eta.hours', { n: h });
  }
  const home = p => String(p || '').replace(/^\/Users\/[^/]+/, '~').replace(/^[A-Z]:\\Users\\[^\\]+\\/i, '');   // ~/… on a Mac; Documents\… on Windows, as Explorer names it
  const baseName = p => String(p || '').split(/[\\/]/).filter(Boolean).pop() || '';

  /* ------------------------------------------------------------ the engine */
  async function call(path, body) {
    const opts = body === undefined ? {} : { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) };
    let r; try { r = await fetch('/api/setup/' + path, opts) } catch { throw Object.assign(new Error(t('setup.err.no_answer')), { status: 0 }) }
    let d = null; try { d = await r.json() } catch { }
    if (!r.ok && !(d && (d.error || d.need_bytes))) throw Object.assign(new Error(r.status === 404 ? t('setup.err.not_available') : t('setup.err.unreadable')), { status: r.status });
    return d;
  }
  const errCode = e => e && (typeof e === 'object' ? e.code : null);
  const errText = e => e && (typeof e === 'object' ? e.message || e.error : e);
  /* What the engine says, as a code with params beside its English (crates/engine/src/setup/api.rs CODES and WHY_CODES,
     hardware.rs AI_CODES; a request setup's route could not read, router.rs SETUP_CODES): each code to its whole key. A code not
     listed here shows the engine's own sentence. */
  const CODE = {   // an answer that refused: {error, code, params}
    request_too_large: 'setup.code.request_too_large', bad_request: 'setup.code.bad_request',
    busy: 'setup.code.busy', pack_unavailable: 'setup.code.pack_unavailable', no_ai_offered: 'setup.code.no_ai_offered',
    model_unavailable: 'setup.code.model_unavailable', map_unavailable: 'setup.code.map_unavailable', addresses_unavailable: 'setup.code.addresses_unavailable',
    choose_library: 'setup.code.choose_library', nothing_to_add: 'setup.code.nothing_to_add', cannot_make_folder: 'setup.code.cannot_make_folder',
    cannot_save_there: 'setup.code.cannot_save_there', too_deep: 'setup.code.too_deep', not_library_there: 'setup.code.not_library_there',
    no_space: 'setup.code.no_space', cannot_save_progress: 'setup.code.cannot_save_progress', nothing_to_repair: 'setup.code.nothing_to_repair',
    repair_unknown: 'setup.code.repair_unknown', missing: 'setup.code.missing', not_library: 'setup.code.not_library',
    library_read_only: 'setup.code.library_read_only', wrong_size: 'setup.code.wrong_size', pack_not_in_version: 'setup.code.pack_not_in_version',
    wrong_size_model: 'setup.code.wrong_size_model', unknown_file: 'setup.code.unknown_file', not_done: 'setup.code.not_done',
    no_ai_to_remove: 'setup.code.no_ai_to_remove', cannot_remove_model: 'setup.code.cannot_remove_model', not_moved_aside: 'setup.code.not_moved_aside',
    not_old_library: 'setup.code.not_old_library',
  };
  const REC_WHY = {   // why setup suggests what it does: recommend.why_codes
    none_ready: 'setup.rec.none_ready', none_fits: 'setup.rec.none_fits', other_disk: 'setup.rec.other_disk', other_disk_network: 'setup.rec.other_disk_network',
    start_small: 'setup.rec.start_small', default_folder: 'setup.rec.default_folder', ai_no_room: 'setup.rec.ai_no_room',
  };
  const AI_WHY = {   // why the local AI fits this computer or not: hardware.ai.why_codes, and after recommend's own reasons
    fits: 'setup.ai.fits', fits_graphics: 'setup.ai.fits_graphics', too_big: 'setup.ai.too_big', ram_unknown: 'setup.ai.ram_unknown',
    none_offered: 'setup.ai.none_offered', measured_fits: 'setup.ai.measured_fits', measured_from_disk: 'setup.ai.measured_from_disk',
    measured_too_big: 'setup.ai.measured_too_big',
  };
  // a code's params as this page shows them: a pack by its name in the person's language, a folder as home() writes it
  function shown(m) {
    const p = Object.assign({}, m && m.params);
    if (typeof p.pack === 'string') p.pack = packTitle(packById(p.pack) || { id: p.pack, title: p.pack });
    if (typeof p.folder === 'string') p.folder = home(p.folder);
    return { code: m && m.code, params: p };
  }
  // an answer that refused, in the person's language (sizes as this system counts them)
  const sayErr = r => I18N.said(CODE, shown(r), r && r.error, { size });
  /* reasons, a list of {code, params}: each in the person's language, memory counted as the tiles above count it. The engine's
     English (why) when there is no list, or it holds a code this page does not know: then the whole stays one language. */
  function sayWhy(codes, english) {
    const known = c => c && (Object.hasOwn(REC_WHY, c.code) || Object.hasOwn(AI_WHY, c.code));
    if (!Array.isArray(codes) || !codes.length || !codes.every(known)) return String(english || '');
    return codes.map(c => Object.hasOwn(AI_WHY, c.code) ? I18N.said(AI_WHY, c, '', { size: ram }) : I18N.said(REC_WHY, shown(c), '', { size })).join(' ');
  }
  /* Pack titles and summaries come from the manifest, in English: shown as they are in English, else in the interface's words */
  const english = () => I18N.lang === 'en';

  /* ------------------------------------------------------------ state */
  let S = null;           // GET state
  let view = 'choose';
  let choice = { pack: null, ai: false, country: null, map: null, place: null };   // country: ISO 3166-1 code; map: the region code (crates/engine/src/regions.rs); place: where a new model goes, 'computer' or 'library' (null: the default)
  let folder = '';
  let missing = { dismissed: false, note: '', opening: false };   // the library remembered from last time cannot be reached (state.missing)
  let shortfall = null;   // the answer to a start that had no room: {need_bytes, free_bytes}
  let pollT = 0;
  let confirmCancel = false, confirmRemove = false, confirmOld = false;
  let note = '', noteOk = false;
  let repair = null;      // a repair under way: {fixed} turns true once it had to download something

  const packs = () => (S.manifest && S.manifest.packs) || [];
  const model = () => ((S.manifest && S.manifest.models) || [])[0] || null;
  const cur = () => S.current || null;
  const packById = id => packs().find(p => p.id === id);
  function packNeedsInstall(pack) {
    if (!pack) return false;
    const current = cur();
    if (!current || pack.id !== current.pack) return true;
    return Boolean(pack.revision && pack.revision !== current.content_revision);
  }
  // The published packs by id: their names and summaries in each language, and whether the name already says "library" (whole: the
  // starter pack, content/library/starter.json), so that it is never said twice. Another id keeps the manifest's own words.
  const PACK = { small: ['setup.pack.small', 'setup.pack.small_summary'], medium: ['setup.pack.medium', 'setup.pack.medium_summary'], all: ['setup.pack.all', 'setup.pack.all_summary'],
    starter: ['setup.pack.starter', 'setup.pack.starter_summary', 'whole'] };
  const packTitle = p => !p ? '' : english() && p.title ? p.title : PACK[p.id] ? t(PACK[p.id][0]) : p.title || p.id;
  const packSummary = p => !p ? '' : english() && p.summary ? p.summary : PACK[p.id] ? t(PACK[p.id][1]) : p.summary || '';
  /* a pack as the library it is: "Small library"; a name that already says library ("Starter library", "Biblioteca di base") stays as
     it is. In English, and for a pack the table does not know, the name itself shows whether it does. */
  const packItem = p => {
    if (!p) return '';
    const name = packTitle(p), whole = english() || !PACK[p.id] ? /library/i.test(name) : PACK[p.id][2] === 'whole';
    return whole ? name : t('setup.item.library', { name });
  };

  /* region maps (docs/SETUP-API.md, Region maps). A region is a country, or for the five largest a part of one: its code is
     LU or DE-BY (crates/engine/src/regions.rs). The person picks the country, then the region; Lastbook never looks for where it is.
     Country names come from Intl in the interface's language; a region keeps its own name (Bayern, Bretagne). */
  const maps = () => ((S.manifest && S.manifest.maps) || []).slice().sort((a, b) => I18N.compare(a.title, b.title));   // by name, as people look for theirs
  const mapById = id => maps().find(m => m.id === id);
  const countryOf = id => String(id || '').split('-')[0];
  const countryName = cc => I18N.country(cc);
  const countries = () => [...new Set(maps().map(m => countryOf(m.id)))].sort((a, b) => I18N.compare(countryName(a), countryName(b)));
  const regionsOf = cc => maps().filter(m => countryOf(m.id) === cc);
  const split = cc => regionsOf(cc).length > 1 || regionsOf(cc).some(m => m.id !== cc);
  const mapName = id => id === 'europe' ? t('setup.map.every_region') : !mapById(id) ? id : split(countryOf(id)) ? t('setup.map.region_country', { region: mapById(id).title, country: countryName(countryOf(id)) }) : countryName(id);
  /* the detailed map the library will have once the chosen pack is in: the person's region map, or the one the pack carries (manifest packs[].map) */
  const packMap = id => (packById(id) || {}).map || null;
  const mapNow = () => { const c = cur() || {}; return choice.pack && choice.pack !== c.pack ? packMap(choice.pack) : (c.map && c.map !== 'base' ? c.map : packMap(c.pack)) };
  const selectedRegion = () => choice.map || (mapNow() !== 'europe' && mapNow()) || ((cur() || {}).address_indexes || [])[0] || '';
  const selectedCountry = () => choice.country || countryOf(selectedRegion()) || '';
  const addresses = () => (S.manifest && S.manifest.address_indexes) || [];
  const addressById = id => addresses().find(a => a.id === id);
  const addressAdds = () => { const a = addressById(selectedRegion()); return a && a.available !== false && !((cur() || {}).address_indexes || []).includes(a.id) ? a : null };
  const mapMissing = () => maps().length > 0 && !selectedRegion();   // no region maps published yet: nothing to choose
  const mapAdds = () => choice.map && choice.map !== mapNow() && mapNow() !== 'europe' ? mapById(choice.map) : null;
  const packDl = p => p.download_bytes || (p.files || []).reduce((n, f) => n + (f.size || 0), 0);
  const SEP = WIN ? '\\' : '/', DATA = 'LastbookData';
  const asData = p => /[\\/]LastbookData[\\/]?$/i.test(p) ? p.replace(/[\\/]$/, '') : String(p).replace(/[\\/]$/, '') + SEP + DATA;
  /* a file already on this computer, of the right size, for a pack, a map, an address index or the model: setup uses it instead of
     downloading (the engine does, for each of them) */
  const foundFor = (kind, id) => (S.found || []).find(f => f.kind === kind && (f.size_ok !== false) && (kind === 'model' || f.id === id || f.pack === id));
  /* a file found here, named as the done page names what it holds (itemTitle: "Map: Liechtenstein", a pack by its own name); a file of
     something the manifest does not list, by its file name */
  const foundTitle = f => f.kind === 'library' ? t('setup.found.library') : f.kind === 'model' ? t('setup.found.model')
    : (f.kind === 'pack' ? packById(f.id) : (f.kind === 'map' || f.kind === 'addresses') && mapById(f.id)) ? itemTitle({ kind: f.kind, id: f.id, title: f.name })
    : f.name || baseName(f.path);
  const aiFits = () => !!(S.hardware && S.hardware.ai && S.hardware.ai.fits);
  // why the local AI does not fit this computer
  const aiWhy = () => { const a = (S.hardware && S.hardware.ai) || {}; return sayWhy(a.why_codes, a.why) };

  function diskFor(path) {
    const ds = (S.hardware && S.hardware.disks) || [];
    const norm = s => WIN ? String(s).toLowerCase().replace(/\//g, '\\') : String(s);
    const p = norm(path);
    let best = null;
    for (const d of ds) if (p.startsWith(norm(d.path)) && (!best || d.path.length > best.path.length)) best = d;
    return best || (path ? null : ds[0] || null);   // a folder on a disk the engine did not report: its free space is not known here
  }
  // "on Macintosh HD", "on D:", "on this Mac": a disk named where something is, as the sentences around it need it
  function diskWhere(d) {
    const name = d && (d.name || (WIN ? (String(d.path).match(/^[A-Z]:/i) || [''])[0].toUpperCase() : /^\/Volumes\/[^/]+/.test(d.path) ? d.path.split('/')[2] : ''));
    return name ? t('setup.disk.on', { name }) : t(!d || WIN ? 'setup.disk.on_this_disk' : /Linux/.test(navigator.platform || '') ? 'setup.disk.on_this_computer' : 'setup.disk.on_this_mac');
  }
  /* Each disk's kind (hardware.disks[].kind, current.disk_kind): this computer's own disk, an external disk or USB stick (it must stay
     connected), or a network share on a NAS (the network must stay on, and it is slower). */
  const KIND = { local: 'setup.disk.kind_local', removable: 'setup.disk.kind_removable', network: 'setup.disk.kind_network' };
  const LIVES = { local: 'setup.disk.lives_local', removable: 'setup.disk.lives_removable', network: 'setup.disk.lives_network' };
  const diskName = d => d.name || baseName(d.path) || d.path;
  // the disks a new library can go on, as the engine lists them (not the places the person picked, which add only their free space)
  const disksShown = () => ((S.hardware && S.hardware.disks) || []).filter(d => !d.picked);

  function plan() {   // what the current choice adds: bytes to download, space it needs (download + unpacked + 2 GB, as the engine counts)
    const c = cur() || {};
    let dl = 0, need = 0;
    const p = packById(choice.pack);
    if (packNeedsInstall(p)) {
      const got = foundFor('pack', p.id);
      if (!got) dl += packDl(p);
      need += (got ? 0 : packDl(p)) + (p.installed_bytes || 0);
      if (p.id === c.pack) need += c.map_bytes || 0;
    }
    const m = model();
    if (choice.ai && m && !c.model && aiFits()) { const got = foundFor('model'); if (!got) { dl += m.size || 0; need += m.size || 0 } else need += m.size || 0 }
    const mp = mapAdds();
    if (mp) { const got = foundFor('map', mp.id); if (!got) dl += packDl(mp); need += (got ? 0 : packDl(mp)) + (mp.installed_bytes || 0) }
    const a = addressAdds();
    if (a) { const got = foundFor('addresses', a.id); if (!got) dl += packDl(a); need += (got ? 0 : packDl(a)) + (a.installed_bytes || 0) }
    if (need) need += 2 * 1000 ** 3;
    return { dl, need };
  }

  async function load() {
    try { S = await call('state') }
    catch (e) { main.innerHTML = `<div class="s-err" role="alert">${ico('warn')}<div><b>${th('setup.err.could_not_start')}</b><p>${esc(e.message)}</p><p><button class="btn" id="again">${th('setup.try_again')}</button></p></div></div>`; $('#again').onclick = load; return }
    const rf = (S.recommend || {}).folder;
    folder = folder || (cur() && cur().folder) || S.default_folder || (rf ? asData(rf) : '');
    if (!choice.pack) {
      const rec = S.recommend || {};
      const p = packById(rec.pack);   // the engine's suggestion: the smallest library available
      choice.pack = p && p.available !== false ? p.id : null;
      choice.ai = !!rec.ai && aiFits();
      if (cur()) { choice.pack = cur().pack; choice.ai = !!cur().model }
    }
    const j = S.job;
    if (j && j.state && j.state !== 'done' && !(j.state === 'failed' && errCode(j.error) === 'cancelled')) view = 'job';
    else if (j && j.state === 'done') view = 'done';
    else if (cur() || S.mode === 'manage' || MANAGE_ASKED) view = cur() ? 'manage' : 'choose';
    $('#sBack').hidden = !cur();
    render(true);
  }

  /* ------------------------------------------------------------ views */
  function render(focus) {
    clearTimeout(pollT);
    shapeKey = '';
    if (view === 'manage') manageView();
    else if (view === 'choose') chooseView();
    else if (view === 'job') { jobView(S.job); poll() }
    else if (view === 'done') doneView();
    if (focus) { const h = main.querySelector('h1'); if (h) { h.tabIndex = -1; h.focus({ preventScroll: true }) } scrollTo(0, 0) }
  }
  function go(v) { view = v; confirmCancel = confirmRemove = confirmOld = false; note = ''; render(true) }

  /* ---- this computer */
  function computer() {
    const h = S.hardware || {}, d = diskFor(folder), tiles = [];
    if (d) tiles.push([ico('disk'), size(d.free_bytes), th('setup.pc.free', { where: diskWhere(d) })]);
    if (h.ram_bytes) tiles.push([ico('chip'), ram(h.ram_bytes), th(h.unified_memory ? 'setup.pc.memory_shared' : 'setup.pc.memory')]);
    if (!h.unified_memory) {
      const g = (h.gpus || []).slice().sort((a, b) => (b.vram_bytes || 0) - (a.vram_bytes || 0))[0];
      tiles.push(g ? [ico('gpu'), g.vram_bytes ? ram(g.vram_bytes) : '—', th('setup.pc.graphics', { name: g.name })] : [ico('gpu'), th('setup.pc.none'), th('setup.pc.no_gpu')]);
    }
    return `<section class="s-pc" aria-label="${th('setup.pc.label')}">${tiles.map(x => `<div class="s-tile">${x[0]}<b>${x[1]}</b><small>${x[2]}</small></div>`).join('')}</section>`;
  }
  const refreshComputer = () => { const pc = main.querySelector('.s-pc'); if (pc) pc.outerHTML = computer() };
  // what setup suggests, and why (recommend.why_codes; the AI's own reasons follow when no model fits)
  function recommendLine() {
    const r = S.recommend || {}, p = packById(r.pack), why = sayWhy(r.why_codes, r.why);
    if (!p) return why ? `<p class="s-rec bad">${ico('warn')}<span>${esc(why)}</span></p>` : '';
    const what = r.ai && aiFits() ? th('setup.rec.pack_with_ai', { pack: packTitle(p) }) : esc(packTitle(p));
    return `<p class="s-rec">${ico('ok')}<span><b>${th('setup.rec.suggested', { what: I18N.raw(what) })}</b> ${esc(why)}</span></p>`;
  }
  /* The library remembered from last time cannot be reached (state.missing): its disk is unplugged or its network share is away.
     Try again uses it if it is back (the app then starts again on it); or choose another library folder, or set up a new one. */
  function missingCard() {
    const m = S.missing;
    if (!m || !m.folder || cur() || missing.dismissed) return '';
    return `<section class="s-missing" aria-labelledby="missH">${ico('warn')}<div>
      <h2 id="missH">${th('setup.missing.title')}</h2>
      <p>${th(m.here ? 'setup.missing.back' : 'setup.missing.text', { folder: home(m.folder) })}</p>
      <p class="s-acts"><button class="btn" id="missTry"${missing.opening ? ' disabled' : ''}>${m.here ? ico('ok') + th('setup.missing.open') : ico('retry') + th('setup.try_again')}</button>
        <button class="btn plain" id="missOther"${missing.opening ? ' disabled' : ''}>${ico('folder')}${th('setup.missing.choose_other')}</button>
        <button class="btn plain" id="missNew"${missing.opening ? ' disabled' : ''}>${th('setup.missing.new')}</button></p>
      <p class="s-note${missing.opening ? ' ok' : ''}" id="missNote"${missing.note ? '' : ' hidden'}>${esc(missing.note)}</p></div></section>`;
  }
  async function missingTry() {
    const b = $('#missTry'); if (b) b.disabled = true;
    const r = await call('use', { path: S.missing.folder }).catch(e => ({ error: e.message }));
    if (!r || r.error) missing.note = r && r.code === 'missing' ? t('setup.missing.still') : (r && sayErr(r)) || t('setup.missing.still');
    else {
      const f = await call('finish', {}).catch(e => ({ error: e.message }));   // on {ok} the app starts again on the library
      if (!f || f.error) missing.note = (f && sayErr(f)) || t('setup.done.open_failed');
      else { missing.note = t('setup.done.opening'); missing.opening = true; live.textContent = missing.note }
    }
    chooseView();
    const n = $('#missNote'); if (n) { n.tabIndex = -1; n.focus() }
  }

  /* ---- choose */
  function chooseView() {
    const c = cur(), m = model();
    const packCards = packs().map(p => {
      const update = c && c.pack === p.id && packNeedsInstall(p);
      const off = p.available === false, here = c && c.pack === p.id && !update, rec = (S.recommend || {}).pack === p.id && !c;
      const badge = off ? `<span class="s-badge later">${th('setup.badge.later')}</span>` : update ? `<span class="s-badge rec">${th('setup.badge.update')}</span>` : here ? `<span class="s-badge here">${th('setup.badge.installed')}</span>` : rec ? `<span class="s-badge rec">${th('setup.badge.suggested')}</span>` : '';
      return `<label class="s-opt${off ? ' off' : ''}">
        <input type="radio" name="pack" value="${esc(p.id)}"${choice.pack === p.id ? ' checked' : ''}${off ? ' disabled' : ''}>
        <span class="s-opt-in"><span class="s-opt-h"><b>${esc(packTitle(p))}</b>${badge}</span>
        <small data-pack-summary="${esc(p.id)}">${esc(packSummary(p))}</small>
        <span class="s-sizes">${off ? th('setup.size.not_ready') : here ? th('setup.size.here') : `<span>${foundFor('pack', p.id) ? th('setup.size.found') : th('setup.size.download', { size: size(packDl(p)) })}</span><span>${th('setup.size.on_disk', { size: size(p.installed_bytes) })}</span>`}</span></span></label>`;
    }).join('');
    let ai = '';
    if (m && m.available !== false) {
      const has = c && c.model;
      ai = aiFits() || has
        ? `<label class="s-opt s-ai"><input type="checkbox" id="aiBox"${choice.ai || has ? ' checked' : ''}${has ? ' disabled' : ''}>
            <span class="s-opt-in"><span class="s-opt-h"><b>${th('setup.ai.title')}</b>${has ? `<span class="s-badge here">${th('setup.badge.installed')}</span>` : (S.recommend || {}).ai && !c ? `<span class="s-badge rec">${th('setup.badge.suggested')}</span>` : `<span class="s-badge">${th('setup.badge.optional')}</span>`}</span>
            <small>${th('setup.ai.blurb')}</small>
            <span class="s-sizes">${has ? th('setup.size.here') : foundFor('model') ? `<span>${th('setup.size.found')}</span>` : `<span>${th('setup.size.about_download', { size: size(m.size) })}</span>`}</span></span></label>`
        : `<div class="s-opt s-ai off" role="group" aria-label="${th('setup.ai.title')}"><span class="s-opt-in"><span class="s-opt-h"><b>${th('setup.ai.title')}</b><span class="s-badge later">${th('setup.badge.not_here')}</span></span>
            <small>${esc(aiWhy())}</small></span></div>`;
    }
    const found = (S.found || []).map((f, i) => f.rejected ? `<div class="s-found off">${ico('file')}<span><b>${esc(foundTitle(f))}</b><small>${th('setup.found.rejected', { path: home(f.path) })}</small></span></div>`
      : f.size_ok === false ? '' : `<div class="s-found">${ico(f.kind === 'library' ? 'folder' : 'file')}<span><b>${esc(foundTitle(f))}</b><small>${esc(home(f.path))}${f.size ? ' · ' + size(f.size) : ''}</small></span>
        <button class="btn plain" data-use="${i}">${th('setup.found.use')}</button></div>`).join('');

    main.innerHTML = `${missingCard()}${c ? `<button class="s-link" id="toManage">${ico('back')}<span>${th('setup.manage.title')}</span></button>` : ''}
      <h1>${c ? th('setup.choose.change_title') : th('setup.choose.title')}</h1>
      ${c ? '' : `<p class="lead">${th('setup.choose.lead')}</p>`}
      ${computer()}
      ${c ? '' : recommendLine()}
      ${found ? `<h2>${th('setup.found.title')}</h2><div class="s-founds">${found}</div>` : ''}
      <section id="mapBox" class="s-map-panel" aria-label="${th('setup.map.title')}"></section>
      <h2 id="packH">${th('setup.choose.library')}</h2>
      <div class="s-opts" role="radiogroup" aria-labelledby="packH">${packCards}</div>
      ${ai ? `<h2>${th('setup.ai.title')}</h2>${ai}<div id="placeBox"></div>` : ''}
      <section class="s-sum" id="sum" aria-live="polite"></section>
      <div class="s-go">
        <button class="btn big" id="start"></button>
        ${c ? '' : `<button class="s-link" id="haveLib">${th('setup.choose.have_library')}</button>`}
        <button class="s-link" id="haveFile">${th('setup.choose.have_file')}</button>
      </div>
      <p class="s-note" id="haveNote" hidden></p>`;
    main.querySelectorAll('input[name=pack]').forEach(r => r.onchange = () => { choice.pack = r.value; shortfall = null; mapSection(); summary() });
    const box = $('#aiBox'); if (box && !box.disabled) box.onchange = () => { choice.ai = box.checked; shortfall = null; placeSection(); summary() };
    main.querySelectorAll('[data-use]').forEach(b => b.onclick = () => useFile((S.found || [])[+b.dataset.use].path, b));
    $('#start').onclick = start;
    if ($('#toManage')) $('#toManage').onclick = () => go('manage');
    if ($('#haveLib')) $('#haveLib').onclick = () => haveLibrary(false);
    $('#haveFile').onclick = () => haveLibrary(true);
    if ($('#missTry')) $('#missTry').onclick = missingTry;
    if ($('#missOther')) $('#missOther').onclick = () => haveLibrary(false);
    if ($('#missNew')) $('#missNew').onclick = () => { missing.dismissed = true; chooseView(); const h = main.querySelector('h1'); h.tabIndex = -1; h.focus() };
    mapSection();
    placeSection();
    summary();
  }
  /* Where a new AI model goes (start's model_place): on this computer, in the app's own folder, or beside the library. A model
     read from a network share or a USB stick loads slowly, so for a library there the engine suggests this computer
     (recommend.model_place); the person decides. */
  const placeAsked = () => !!(S.recommend || {}).model_place && !!model() && choice.ai && aiFits() && !(cur() || {}).model;
  function place() {
    if (choice.place) return choice.place;
    const r = S.recommend || {}, d = diskFor(folder);
    if (cur() || !d || !d.kind || (r.folder && asData(r.folder) === folder)) return r.model_place || 'library';
    return d.kind === 'local' ? 'library' : 'computer';   // another folder than the suggested one: the engine's rule, for its disk
  }
  function placeSection() {
    const el = $('#placeBox'); if (!el) return;
    const p = place();
    el.innerHTML = !placeAsked() ? '' : `<fieldset class="s-place"><legend>${th('setup.place.title')}</legend><div class="s-list">
      <label class="s-opt"><input type="radio" name="place" value="computer"${p === 'computer' ? ' checked' : ''}><span class="s-opt-in"><b>${th('setup.place.computer')}</b><small>${th('setup.place.computer_note')}</small></span></label>
      <label class="s-opt"><input type="radio" name="place" value="library"${p === 'library' ? ' checked' : ''}><span class="s-opt-in"><b>${th('setup.place.library')}</b><small>${th('setup.place.library_note')}</small></span></label></div>
      <p class="s-hint">${th('setup.place.slow')}</p></fieldset>`;
    el.querySelectorAll('input[name=place]').forEach(r => r.onchange = () => { choice.place = r.value });
  }
  /* Map geometry and address indexes are separate downloads, chosen by region: the country first, then, in a country made of
     several regions, the region. A country that is one region needs one choice only. */
  function mapSection() {
    const box = $('#mapBox'), now = mapNow();
    if (!box) return;
    box.hidden = !maps().length;
    if (box.hidden) return;
    main.querySelectorAll('[data-pack-summary]').forEach(el => { el.textContent = packSummary(packById(el.dataset.packSummary)) });
    const selected = selectedRegion(), cc = selectedCountry();
    /* what choosing region m adds, [kind, entry]: its map unless the library has it, its address index unless the library has it.
       Of that, what still downloads: a file already on this computer (found[]) is used instead, as plan() counts it. */
    const adds = m => {
      const a = addressById(m.id), out = [];
      if (!(now === 'europe' || m.id === now)) out.push(['map', m]);
      if (a && a.available !== false && !((cur() || {}).address_indexes || []).includes(m.id)) out.push(['addresses', a]);
      return out;
    };
    const dlOf = add => add.reduce((n, [kind, x]) => n + (foundFor(kind, x.id) ? 0 : packDl(x)), 0);
    const usesFound = add => add.some(([kind, x]) => foundFor(kind, x.id));
    const offFor = m => !(now === 'europe' || m.id === now) && m.available === false;
    const details = m => { const add = adds(m), dl = dlOf(add);
      return offFor(m) ? t('setup.map.later') : dl ? t('setup.size.download', { size: size(dl) }) : usesFound(add) ? t('setup.map.found') : t('setup.map.included') };
    const countryOpts = countries().map(k => {
      const rs = regionsOf(k), one = !split(k) ? rs[0] : null;
      const off = rs.every(offFor);
      return `<option value="${esc(k)}"${cc === k ? ' selected' : ''}${off ? ' disabled' : ''}>${esc(countryName(k))}${one ? ' — ' + esc(details(one)) : off ? ' — ' + th('setup.map.later') : ''}</option>`;
    }).join('');
    const regionOpts = cc && split(cc) ? regionsOf(cc).map(m => `<option value="${esc(m.id)}"${selected === m.id ? ' selected' : ''}${offFor(m) ? ' disabled' : ''}>${esc(m.title)} — ${esc(details(m))}</option>`).join('') : '';
    const sel = mapById(selected), add = sel ? adds(sel) : [], dl = dlOf(add), found = usesFound(add);
    const mapIncluded = !add.some(([kind]) => kind === 'map'), a = add.some(([kind]) => kind === 'addresses');
    const disk = add.reduce((n, [, x]) => n + (x.installed_bytes || 0), 0);
    const unavailable = selected && addresses().length && (!addressById(selected) || addressById(selected).available === false);
    const sizes = { download: size(dl), disk: size(disk) };
    // its sizes: what downloads and the room it takes; with nothing to download, what is already here, else all included
    let hint = !cc ? t('setup.map.pick_country_hint') : !sel ? t('setup.map.pick_region_hint')
      : dl ? t(mapIncluded && a ? 'setup.map.sizes_addresses' : 'setup.map.sizes', sizes) : found ? t('setup.map.sizes_found', sizes) : t('setup.map.all_included');
    if (sel && unavailable) hint = (dl ? t('setup.map.sizes', sizes) + '.' : found ? t('setup.map.sizes_found', sizes) + '.' : t('setup.map.map_included')) + ' ' + t('setup.map.no_addresses');
    box.innerHTML = `<h2>${ico('map')}${th('setup.map.title')}</h2>
      <p class="s-mapnote">${th('setup.map.note')}</p>
      ${now === 'europe' ? `<p class="s-mapnote">${th('setup.map.europe_note')}</p>` : ''}
      <label class="s-maplabel" for="mapCountry">${th('setup.map.country')}</label>
      <select id="mapCountry" class="s-select" required aria-describedby="mapHint"><option value="" disabled${cc ? '' : ' selected'}>${th('setup.map.select_country')}</option>${countryOpts}</select>
      ${regionOpts ? `<label class="s-maplabel" for="mapSel">${th('setup.map.region')}</label>
      <select id="mapSel" class="s-select" required aria-describedby="mapHint"><option value="" disabled${selected && countryOf(selected) === cc ? '' : ' selected'}>${th('setup.map.select_region')}</option>${regionOpts}</select>` : ''}
      <p class="s-hint" id="mapHint">${esc(hint)}</p>`;
    $('#mapCountry').onchange = e => {
      choice.country = e.target.value;
      choice.map = split(choice.country) ? null : (regionsOf(choice.country)[0] || {}).id || null;
      shortfall = null; mapSection(); summary(); ($('#mapSel') || $('#mapCountry')).focus();
    };
    if ($('#mapSel')) $('#mapSel').onchange = e => { choice.map = e.target.value; shortfall = null; mapSection(); summary(); $('#mapSel').focus() };
  }
  function summary() {
    const { dl, need } = plan(), d = diskFor(folder), free = d ? d.free_bytes : null;
    const short = shortfall || (need && free != null && free < need ? { need_bytes: need, free_bytes: free } : null);
    const pct = need && free ? Math.min(100, need / free * 100) : short ? 100 : 0;
    const c = cur();
    $('#sum').innerHTML = need ? `
      <div class="s-nums"><span><small>${th('setup.sum.download')}</small><b>${dl ? size(dl) : th('setup.sum.none')}</b></span><span><small>${th('setup.sum.space')}</small><b>${size(short ? short.need_bytes : need)}</b></span><span><small>${th('setup.sum.free', { where: diskWhere(d) })}</small><b>${short ? size(short.free_bytes) : free != null ? size(free) : th('setup.sum.checked_at_start')}</b></span></div>
      ${free == null && !short ? '' : `<div class="s-meter${short ? ' bad' : ''}" role="img" aria-label="${th('setup.sum.meter', { pct: Math.round(pct) })}"><i style="width:${pct}%"></i></div>`}
      ${short ? `<p class="s-short">${ico('warn')}<span>${th('setup.sum.short', { need: size(short.need_bytes), free: size(short.free_bytes), where: diskWhere(d) })}</span></p>` : ''}
      ${disksRow()}${folderRow()}`
      : `<p class="s-nothing">${c ? th('setup.sum.have_now') : th('setup.sum.pick')}</p>${c ? '' : disksRow() + folderRow()}`;
    main.querySelectorAll('input[name=disk]').forEach(r => r.onchange = () => {
      folder = asData(disksShown()[+r.value].path); shortfall = null;
      refreshComputer(); placeSection(); summary();
      const again = main.querySelector(`input[name=disk][value="${r.value}"]`); if (again) again.focus();
    });
    const b = $('#start');
    b.disabled = !need || !!short || mapMissing();
    const newPack = c && packNeedsInstall(packById(choice.pack)), newAi = c && choice.ai && !c.model, newMap = c && mapAdds(), newAddresses = c && addressAdds();
    const parts = [newPack && packTitle(packById(choice.pack)), newAi && t('setup.go.part_ai'), newMap && t('setup.go.part_map', { name: mapName(newMap.id) }), newAddresses && !newMap && t('setup.go.part_addresses')].filter(Boolean);
    // with nothing to download (every file found on this computer), the button says only that it installs
    b.textContent = mapMissing() ? t('setup.go.choose_region') : !c ? t(dl ? 'setup.go.install' : 'setup.go.install_found')
      : parts.length ? t(dl ? 'setup.go.get' : 'setup.go.install_list', { list: I18N.list(parts) }) : t('setup.go.nothing');
    if (newPack && choice.pack === c.pack && !mapMissing()) b.textContent = t(dl ? 'setup.go.install_update' : 'setup.go.install_update_found');
    const ch = $('#chFolder'); if (ch) ch.onclick = pickFolder;
  }
  // where the library goes; on an external disk, a USB stick or a network share, said plainly beside it
  const folderRow = () => {
    const k = (diskFor(folder) || {}).kind;
    return `<div class="s-folder">${ico('folder')}<span><small>${th('setup.folder.saves_to')}</small><b title="${esc(folder)}">${esc(home(folder))}</b></span>${cur() ? '' : `<button class="btn plain" id="chFolder">${th('setup.folder.change')}</button>`}</div>
      ${k === 'removable' || k === 'network' ? `<p class="s-kind">${ico(k === 'network' ? 'nas' : 'usb')}<span>${th(k === 'network' ? 'setup.disk.saves_network' : 'setup.disk.saves_removable')}</span></p>` : ''}`;
  };
  /* The disks a new library can go on, each with its kind; choosing one puts the library at its top, in LastbookData. Shown when
     there is more than one; Change… picks any folder. */
  function disksRow() {
    const ds = disksShown();
    if (cur() || ds.length < 2) return '';
    const at = diskFor(folder), on = at && !at.picked ? at : at && ds.find(d => d.name && d.name === at.name);
    return `<fieldset class="s-disks"><legend>${th('setup.disk.title')}</legend><div class="s-list">${ds.map((d, i) => `<label class="s-disk">
        <input type="radio" name="disk" value="${i}"${d === on ? ' checked' : ''}>${ico(d.kind === 'network' ? 'nas' : d.kind === 'removable' ? 'usb' : 'disk')}
        <span><b>${esc(diskName(d))}</b><small>${th(KIND[d.kind] || KIND.local)}</small></span>
        <span class="s-disk-free">${th('setup.disk.free', { size: size(d.free_bytes) })}</span></label>`).join('')}</div></fieldset>`;
  }

  async function pickFolder() {
    const b = $('#chFolder'); b.disabled = true;
    try {
      const r = await call('pick', {});
      if (r && r.path) {
        folder = asData(r.path);
        if (r.free_bytes != null) (S.hardware.disks = S.hardware.disks || []).push({ path: r.path, name: r.disk, free_bytes: r.free_bytes, kind: r.kind, picked: true });   // the chosen place's own disk
        shortfall = null;
        refreshComputer(); placeSection();
      }
      summary();
    } catch { b.disabled = false; b.textContent = t('setup.folder.cant_change') }
  }
  async function haveLibrary(files) {
    const n = $('#haveNote');
    try {
      const r = await call('pick', { files });
      if (r && r.path) return useFile(r.path, $(files ? '#haveFile' : '#haveLib'));
      if (r && (r.cancelled || r.path === null)) return;
      throw 0;
    } catch {
      n.hidden = false;
      n.textContent = t('setup.pick.failed');
    }
  }
  async function useFile(path, btn) {
    if (btn) btn.disabled = true;
    try {
      const r = await call('use', { path });
      if (r && r.error) throw new Error(sayErr(r));
      if (r && r.noted) { S = await call('state').catch(() => S); choice.ai = true; chooseView(); const n = $('#haveNote'); n.hidden = false; n.textContent = t('setup.use.model_noted'); return }
      S.job = r; go(r.state === 'done' ? 'done' : 'job'); poll();
    } catch (e) {
      if (btn) btn.disabled = false;
      const n = $('#haveNote'); n.hidden = false; n.textContent = e.message;
    }
  }
  async function start() {
    if (mapMissing()) { ($('#mapSel') || $('#mapCountry'))?.focus(); return }
    const b = $('#start'); b.disabled = true;
    const r = await call('start', { pack: packNeedsInstall(packById(choice.pack)) ? choice.pack : null, ai: !!choice.ai && aiFits() && !(cur() || {}).model, map: mapAdds() ? choice.map : null, address_id: addressById(selectedRegion())?.available !== false && addressById(selectedRegion()) ? selectedRegion() : null, folder,
      model_place: placeAsked() ? place() : undefined }).catch(e => ({ error: e.message }));
    if (r && r.need_bytes) { shortfall = r; summary(); return }
    if (!r || r.error) { b.disabled = false; const n = $('#haveNote'); n.hidden = false; n.textContent = (r && r.error && sayErr(r)) || t('setup.err.could_not_start'); return }
    S.job = r; go('job'); poll();
  }

  /* ---- manage */
  function manageView() {
    const c = cur(), p = packById(c.pack), m = model();
    const libraryUpdate = p && p.available !== false && packNeedsInstall(p);
    const o = [].concat(c.old || [])[0], old = o ? (typeof o === 'string' ? { path: o } : o) : null;   // the engine lists the paths it moved aside
    const larger = packs().filter(x => (x.installed_bytes || 0) > ((p && p.installed_bytes) || 0));
    const bigger = larger.filter(x => x.available !== false), later = larger.find(x => x.available === false);
    main.innerHTML = `<h1>${th('setup.manage.title')}</h1>
      <div class="s-have">
        <div class="s-row">${ico('book', 's-ri')}<span><b>${p ? esc(packItem(p)) : c.pack ? esc(packItem({ id: c.pack })) : th('setup.manage.library')}</b><small>${libraryUpdate ? th('setup.manage.update_available') + ' · ' : p ? size(p.installed_bytes) + ' · ' : ''}${esc(home(c.folder))}</small>${LIVES[c.disk_kind] ? `<small class="s-where${c.disk_kind === 'local' ? '' : ' s-ext'}">${ico(c.disk_kind === 'network' ? 'nas' : c.disk_kind === 'removable' ? 'usb' : 'disk')}${th(LIVES[c.disk_kind])}</small>` : ''}</span>
          ${libraryUpdate ? `<button class="btn" id="libraryUpdate">${th('setup.manage.update', { name: packTitle(p) })}</button>` : bigger.length ? `<button class="btn" id="upgrade">${ico('up')}${th('setup.manage.get', { name: packTitle(bigger[0]) })}</button>` : later ? `<span class="s-badge later">${th('setup.manage.comes_later', { name: packTitle(later) })}</span>` : `<span class="s-badge here">${th('setup.manage.largest')}</span>`}</div>
        ${m && m.available !== false ? `<div class="s-row">${ico('ai', 's-ri')}<span><b>${th('setup.ai.title')}</b><small>${c.model ? th('setup.manage.ai_here', { size: size(m.size) }) : aiFits() ? th('setup.manage.ai_absent', { size: size(m.size) }) : esc(aiWhy())}</small></span>
          ${c.model ? (!CAN.removeAi ? '' : confirmRemove ? `<span class="s-confirm"><button class="btn danger" id="rmYes">${th('setup.manage.remove')}</button><button class="btn plain" id="rmNo">${th('setup.manage.keep')}</button></span>` : `<button class="btn plain" id="rmAi">${th('setup.manage.remove')}</button>`) : aiFits() ? `<button class="btn" id="addAi">${th('setup.manage.add')}</button>` : ''}</div>` : ''}
        ${maps().length ? (() => { const mNow = c.map && c.map !== 'base' ? c.map : packMap(c.pack);
          return `<div class="s-row">${ico('map', 's-ri')}<span><b>${mNow ? th('setup.manage.map_named', { name: mapName(mNow) }) : th('setup.manage.map')}</b><small>${mNow ? (mNow === 'europe' ? th('setup.manage.map_everywhere') : th('setup.manage.map_local')) : th('setup.manage.map_none')}</small></span>
          <button class="btn plain" id="mapCh">${(c.address_indexes || []).length ? th('setup.manage.choose_region') : th('setup.manage.add_addresses')}</button></div>` })() : ''}
      </div>
      ${old ? `<div class="s-row s-old">${ico('folder', 's-ri')}<span><b>${th('setup.manage.old')}</b><small>${esc(home(old.path))}${old.bytes ? ' · ' + size(old.bytes) : ''}</small></span>${confirmOld ? `<span class="s-confirm"><span>${th('setup.manage.old_confirm')}</span><button class="btn danger" id="oldYes">${th('setup.manage.delete')}</button><button class="btn plain" id="oldNo">${th('setup.manage.keep')}</button></span>` : `<button class="btn plain" id="rmOld">${ico('trash')}${th('setup.manage.delete_it')}</button>`}</div>` : ''}
      ${computer()}
      ${CAN.repair ? `<div class="s-go left"><button class="btn plain" id="repair">${ico('repair')}${th('setup.manage.repair')}</button></div>` : ''}
      <p class="s-note${noteOk ? ' ok' : ''}" id="haveNote"${note ? '' : ' hidden'}>${esc(note)}</p>`;
    if ($('#libraryUpdate')) $('#libraryUpdate').onclick = () => { choice.pack = c.pack; choice.ai = !!c.model; go('choose') };
    if ($('#upgrade')) $('#upgrade').onclick = () => { choice.pack = bigger[0].id; choice.ai = !!c.model; go('choose') };
    if ($('#mapCh')) $('#mapCh').onclick = () => { choice.pack = c.pack; choice.ai = !!c.model; choice.map = null; choice.country = null; go('choose'); const m = $('#mapCountry'); if (m) m.focus() };
    if ($('#addAi')) $('#addAi').onclick = () => { choice.pack = c.pack; choice.ai = true; go('choose') };
    if ($('#rmAi')) $('#rmAi').onclick = () => { confirmRemove = true; manageView(); $('#rmNo').focus() };
    if ($('#rmNo')) $('#rmNo').onclick = () => { confirmRemove = false; manageView(); $('#rmAi').focus() };
    if ($('#rmYes')) $('#rmYes').onclick = () => act('remove-ai', t('setup.manage.ai_removed'));   // the engine stops the AI first
    if ($('#rmOld')) $('#rmOld').onclick = () => { confirmOld = true; manageView(); $('#oldNo').focus() };
    if ($('#oldNo')) $('#oldNo').onclick = () => { confirmOld = false; manageView(); $('#rmOld').focus() };
    if ($('#oldYes')) $('#oldYes').onclick = () => act('delete-old', t('setup.manage.old_deleting'), { path: old.path });
    if ($('#repair')) $('#repair').onclick = async () => {
      const r = await call('start', { repair: true, ai: !!c.model }).catch(e => ({ error: e.message }));
      if (!r || r.error) { note = (r && r.error && sayErr(r)) || t('setup.err.repair_could_not_start'); noteOk = false; manageView(); return }
      repair = { fixed: false }; S.job = r; go('job'); poll();
    };
  }
  async function act(path, done, body) {
    const r = await call(path, body || {}).catch(e => ({ error: e.status === 404 ? t('setup.err.cannot_yet') : e.message }));
    confirmRemove = confirmOld = false;
    noteOk = !(r && r.error);
    if (r && r.error) note = sayErr(r);
    else { note = done; S = await call('state').catch(() => S) }
    manageView();
    const n = $('#haveNote'); if (n) { n.tabIndex = -1; n.focus() }
  }

  /* ---- job */
  const STEPS = { pack: [['downloading', 'setup.step.download'], ['verifying', 'setup.step.check'], ['unpacking', 'setup.step.unpack'], ['activating', 'setup.step.finish']],
                  model: [['downloading', 'setup.step.download'], ['verifying', 'setup.step.check'], ['activating', 'setup.step.finish']] };
  STEPS.map = [['downloading', 'setup.step.download'], ['unpacking', 'setup.step.unpack']];   // the engine's map item: waiting, downloading, unpacking, done
  STEPS.addresses = STEPS.map;
  const WORD = { waiting: 'setup.state.waiting', downloading: 'setup.state.downloading', paused: 'setup.state.paused', verifying: 'setup.state.verifying', unpacking: 'setup.state.unpacking', activating: 'setup.state.activating', done: 'setup.state.done', failed: 'setup.state.failed' };
  const ACTIVE = new Set(['waiting', 'downloading', 'verifying', 'unpacking', 'activating']);
  let shapeKey = '', lastAct = null;   // lastAct: the button just pressed, so focus lands on what replaced it

  /* A job's error {code, message, params} (crates/engine/src/setup/api.rs JOB_CODES), said by its code with its params */
  function errorBox(j) {
    const e = j.error, code = errCode(e) || j.code, c = cur(), p = (e && typeof e === 'object' && e.params) || {};
    const safe = c ? th('setup.job.safe_library') : th('setup.job.safe_nothing');
    const d = diskFor(j.folder || folder);
    const T = {
      found_mismatch: (() => {   // a file of the person's own that did not match: nothing was downloaded, and their file is left alone
        const f = String((e && e.file) || ''), name = p.name || f.split(/[\\/]/).pop(), at = p.folder || f.split(/[\\/]/).slice(-2, -1)[0];
        return [name ? th(at ? 'setup.job.mismatch_in' : 'setup.job.mismatch', { name, folder: at }) : th('setup.job.mismatch_plain'), th('setup.job.mismatch_next'), 'download'];
      })(),
      no_space: (() => {   // the room it needs and the room there is, when the engine counted them
        const need = p.need_bytes ?? (e && e.need_bytes), free = p.free_bytes ?? (e && e.free_bytes);
        return [need && free != null ? th('setup.job.no_space_need', { where: diskWhere(d), need: size(need), free: size(free) }) : th('setup.job.no_space', { where: diskWhere(d) }), th('setup.job.no_space_next'), 'retry'];
      })(),
      network: [th('setup.job.network'), th('setup.job.network_next'), 'retry'],
      checksum: [th('setup.job.checksum'), th('setup.job.checksum_next'), 'retry'],
      unpack: [th('setup.job.unpack'), th('setup.job.unpack_next'), 'retry'],
      activate: [th('setup.job.activate'), th('setup.job.activate_next'), 'retry'],
      folder: [th('setup.job.folder'), th('setup.job.folder_next'), 'retry'],
      cancelled: [th('setup.job.cancelled'), th('setup.job.cancelled_next'), 'back'],
    }[code] || [esc(errText(e) || t('setup.job.failed')), th('setup.job.failed_next'), 'retry'];
    return `<div class="s-err" role="alert">${ico('warn')}<div><b>${T[0]}</b><p>${T[1]}${code === 'found_mismatch' && !c ? '' : ' ' + safe}</p>
      <p class="s-acts">${T[2] === 'retry' ? `<button class="btn" data-act="retry">${ico('retry')}${th('setup.job.retry')}</button>` : T[2] === 'download' ? `<button class="btn" data-act="retry">${ico('down')}${th('setup.job.download_it')}</button>` : ''}<button class="btn ${T[2] === 'back' ? '' : 'plain'}" data-act="back">${th('setup.job.choose_again')}</button></p></div></div>`;
  }
  function itemHTML(it, j) {
    const FAILED_AT = { checksum: 'verifying', found_mismatch: 'verifying', unpack: 'unpacking', activate: 'activating' };
    const steps = STEPS[it.kind] || STEPS.pack;
    const st = it.state === 'paused' && noDownload(it) ? (it.paused_from && it.paused_from !== 'downloading' ? it.paused_from : 'verifying')
      : it.state === 'paused' || it.state === 'waiting' ? 'downloading' : it.state === 'failed' ? FAILED_AT[errCode(it.error) || errCode(j.error)] || 'downloading' : it.state;
    const at = it.state === 'done' ? steps.length : (it.state === 'waiting' || it.state === 'paused') && !it.bytes_done && !noDownload(it) ? -1 : Math.max(0, steps.findIndex(s => s[0] === st));
    const busy = ['verifying', 'activating'].includes(it.state) || (it.state === 'unpacking' && !it.bytes_total);
    return `<div class="s-item${it.state === 'failed' ? ' bad' : ''}${it.state === 'done' ? ' ok' : ''}" data-item="${esc(it.id)}" data-kind="${esc(it.kind)}">
      <div class="s-item-h">${ico(it.kind === 'model' ? 'ai' : it.kind === 'map' ? 'map' : 'book', 's-ri')}<b>${esc(itemTitle(it))}</b><span class="s-state" data-f="word">${it.state === 'done' ? ico('ok') : ''}${Object.hasOwn(WORD, it.state) ? th(WORD[it.state]) : ''}</span></div>
      <div class="s-bar new${busy && j.state !== 'paused' ? ' busy' : ''}" role="progressbar" aria-label="${esc(itemTitle(it))}" aria-valuemin="0" aria-valuemax="100" data-f="bar"><i></i></div>
      <p class="s-line" data-f="line"></p>
      <ol class="s-steps" aria-label="${th('setup.job.steps')}">${steps.map((s, i) => `<li class="${i < at ? 'done' : i === at && it.state !== 'failed' ? 'now' : i === at ? 'stop' : ''}">${th(s[1])}</li>`).join('')}</ol>
    </div>`;
  }
  // an item's name: the local AI, a region's map or address search, a library (by the pack's name; the engine's title for a library of the person's own)
  const itemTitle = it => it.kind === 'model' ? t('setup.ai.title') : it.kind === 'map' ? t('setup.item.map', { name: mapById(it.id) ? mapName(it.id) : it.title || it.id })
    : it.kind === 'addresses' ? (mapById(it.id) ? t('setup.item.addresses', { name: mapName(it.id) }) : it.title) : it.id === 'library' || it.id === 'repair' ? it.title
    : packItem(packById(it.id) || { id: it.id, title: it.title });
  /* an item that needs nothing from the internet: the person's own file, or one stopped after its download was complete.
     source and paused_from come from the engine (docs/SETUP-API.md); without them, a complete download still counts */
  const noDownload = it => it.source === 'found' || (!!it.paused_from && it.paused_from !== 'downloading') || (it.bytes_total > 0 && it.bytes_done >= it.bytes_total);
  const STOPPED = { verifying: 'setup.line.checking_stopped', unpacking: 'setup.line.unpacking_stopped', activating: 'setup.line.finishing_stopped' };
  const behind = it => { const items = (S.job && S.job.items) || [], i = items.findIndex(x => x.id === it.id && x.kind === it.kind); return items.slice(0, i).some(x => x.state !== 'done' && !noDownload(x)) };
  function fillItem(el, it) {
    const pct = it.bytes_total ? Math.min(100, it.bytes_done / it.bytes_total * 100) : it.state === 'done' ? 100 : 0;
    const bar = el.querySelector('[data-f=bar]'), line = el.querySelector('[data-f=line]');
    const showPct = !bar.classList.contains('busy');
    bar.firstChild.style.width = (showPct ? pct : 100) + '%';
    if (showPct) bar.setAttribute('aria-valuenow', Math.round(pct)); else bar.removeAttribute('aria-valuenow');
    const of = () => t('setup.line.of', { done: size(it.bytes_done), total: size(it.bytes_total) });
    let msg = '';
    if (it.state === 'done') msg = it.bytes_total ? size(it.bytes_total) : '';
    else if (it.state === 'downloading') msg = [of(), rate(it.rate_bps), eta(it.eta_s)].filter(Boolean).join(' · ');
    else if (it.state === 'paused' && noDownload(it)) {
      const what = t(STOPPED[it.paused_from] || 'setup.line.stopped');
      msg = it.paused_from === 'unpacking' && it.bytes_done && it.bytes_total ? t('setup.line.stopped_at', { what, done: size(it.bytes_done), total: size(it.bytes_total) }) : t('setup.line.stopped_nothing', { what });
    }
    else if ((it.state === 'waiting' || it.state === 'paused') && !it.bytes_done) msg = it.bytes_total ? t(behind(it) ? 'setup.line.to_download_after' : 'setup.line.to_download', { size: size(it.bytes_total) }) : '';
    else if (it.state === 'paused' || it.state === 'waiting' || it.state === 'failed') msg = it.bytes_total && it.bytes_done ? of() : '';
    else if (it.state === 'verifying') msg = t('setup.line.verifying');
    else if (it.state === 'unpacking') msg = it.bytes_total ? t('setup.line.unpacking', { done: size(it.bytes_done), total: size(it.bytes_total) }) + (it.eta_s != null ? ' · ' + eta(it.eta_s) : '') : t('setup.line.unpacking_plain');
    else if (it.state === 'activating') msg = it.kind === 'map' ? t('setup.line.switching_map') : t('setup.line.switching_library');
    line.textContent = msg;
  }
  function jobView(j) {
    if (!j) { go(cur() ? 'manage' : 'choose'); return }
    if (j.state === 'done') { go('done'); return }
    const key = j.state + '|' + errCode(j.error) + '|' + confirmCancel + '|' + j.items.map(i => i.kind + ':' + i.id + ':' + i.state).join(',');
    const head = t({ paused: 'setup.job.paused', failed: errCode(j.error) === 'cancelled' ? 'setup.job.cancelled_head' : 'setup.job.stopped' }[j.state] || (repair && !repair.fixed ? 'setup.job.checking' : repair ? 'setup.job.repairing' : 'setup.job.installing'));
    if (key !== shapeKey) {
      const had = lastAct || (document.activeElement && document.activeElement.dataset ? document.activeElement.dataset.act : null);
      lastAct = null;
      const first = !shapeKey;
      shapeKey = key;
      const quiet = j.state === 'paused' && j.items.filter(i => i.state === 'paused').every(noDownload);
      const lead = quiet ? th('setup.job.lead_quiet')
        : j.state === 'paused' ? th('setup.job.lead_paused')
        : j.state === 'failed' ? '' : th('setup.job.lead_running');
      main.innerHTML = `<h1>${esc(head)}</h1>${lead ? `<p class="lead">${lead}</p>` : ''}
        ${j.state === 'failed' ? errorBox(j) : ''}
        <div class="s-items">${j.items.map(it => itemHTML(it, j)).join('')}</div>
        ${j.state === 'failed' ? '' : `<div class="s-go left">${confirmCancel
          ? `<span class="s-confirm"><span>${th('setup.job.cancel_confirm')}</span><button class="btn danger" data-act="cancel-yes">${th('setup.job.stop')}</button><button class="btn plain" data-act="cancel-no">${th('setup.job.keep_going')}</button></span>`
          : `${j.state === 'paused' ? `<button class="btn" data-act="resume">${ico('play')}${th('setup.job.resume')}</button>` : j.state === 'downloading' || j.state === 'waiting' ? `<button class="btn plain" data-act="pause">${ico('pause')}${th('setup.job.pause')}</button>` : ''}
             <button class="btn plain" data-act="cancel">${ico('x')}${th('setup.job.cancel')}</button>`}</div>`}`;
      main.querySelectorAll('[data-act]').forEach(b => b.onclick = () => jobAct(b.dataset.act, b));
      const again = had && main.querySelector(`[data-act="${had}"]`) || (had && main.querySelector('[data-act]'));
      if (again) again.focus();
      else if (first) { const h = main.querySelector('h1'); h.tabIndex = -1; h.focus({ preventScroll: true }) }
      const now = j.items.find(i => ACTIVE.has(i.state));
      const s = j.state === 'failed' ? '' : now ? t('setup.job.live', { head, state: t(WORD[now.state]), item: itemTitle(now) }) : head;
      if (s && live.textContent !== s) live.textContent = s;
    }
    const fresh = main.querySelectorAll('.s-bar.new');
    j.items.forEach(it => { const el = main.querySelector(`[data-item="${CSS.escape(it.id)}"][data-kind="${CSS.escape(it.kind)}"]`); if (el) fillItem(el, it) });
    if (fresh.length) { void main.offsetWidth; fresh.forEach(b => b.classList.remove('new')) }   // a new screen shows the bar where it is, without growing from nothing
  }
  async function jobAct(a, b) {
    if (a === 'back') { S = await call('state').catch(() => S); S.job = null; shortfall = null; go('choose'); return }   // fresh: found[] may now mark a file as not matching
    if (a === 'cancel') { confirmCancel = true; jobView(S.job); main.querySelector('[data-act="cancel-no"]').focus(); return }
    if (a === 'cancel-no') { confirmCancel = false; jobView(S.job); return }
    if (a === 'cancel-yes') { confirmCancel = false; a = 'cancel' }
    b.disabled = true; lastAct = a;
    try { const j = await call(a, {}); if (j && j.items) S.job = j } catch { }
    if (a === 'cancel') { S = await call('state').catch(() => S); S.job = null; go(cur() ? 'manage' : 'choose'); live.textContent = t('setup.job.cancelled'); return }
    jobView(S.job);
    poll();
  }
  function poll() {
    clearTimeout(pollT);
    if (!S.job || !ACTIVE.has(S.job.state)) return;
    pollT = setTimeout(async () => {
      try { const j = await call('job'); if (j) S.job = j } catch { pollT = setTimeout(poll, 3000); return }
      if (repair && S.job && S.job.items.some(i => i.state === 'downloading' || i.state === 'unpacking')) repair.fixed = true;
      if (view !== 'job') return;
      if (S.job && S.job.state === 'done') { go('done'); return }
      jobView(S.job);
      poll();
    }, 1000);
  }

  /* ---- done */
  function doneView() {
    const j = S.job || { items: [] }, fixed = repair && repair.fixed, checked = repair && !fixed;
    const got = j.items.filter(i => i.id !== 'repair' && i.id !== 'library').map(itemTitle);
    main.innerHTML = `<div class="s-done">${ico('ok', 's-big')}<h1>${checked ? th('setup.done.all_good') : fixed ? th('setup.done.repaired') : th('setup.done.ready')}</h1>
      <p class="lead">${checked ? th('setup.done.checked') : fixed ? th('setup.done.fixed') : got.length ? th('setup.done.installed', { list: I18N.list(got) }) : th('setup.done.installed_plain')}</p>
      <button class="btn big" id="open">${th('setup.done.open')}</button><p class="s-note" id="openNote" hidden></p></div>`;
    live.textContent = checked ? t('setup.done.live_checked') : t('setup.done.live_ready');
    $('#open').onclick = async () => {
      const b = $('#open'); b.disabled = true; b.textContent = t('setup.done.opening');
      const r = await call('finish', {}).catch(e => ({ error: e.message }));   // on {ok} the app relaunches itself into the library
      if (!r || r.error) { b.disabled = false; b.textContent = t('setup.done.open'); const n = $('#openNote'); n.hidden = false; n.textContent = (r && r.error && sayErr(r)) || t('setup.done.open_failed') }
      else live.textContent = t('setup.done.opening');
    };
  }

  /* keyboard: Escape closes a question that is open */
  addEventListener('keydown', e => {
    if (e.key !== 'Escape') return;
    if (confirmCancel) { confirmCancel = false; jobView(S.job); const b = main.querySelector('[data-act="cancel"]'); if (b) b.focus() }
    else if (confirmRemove || confirmOld) { const f = confirmRemove ? '#rmAi' : '#rmOld'; confirmRemove = confirmOld = false; manageView(); $(f).focus() }
  });
  /* the window comes back to the front: the job may have moved on, or a restart paused it */
  addEventListener('focus', () => { if (view === 'job') call('job').then(j => { if (j) { S.job = j; jobView(j); poll() } }).catch(() => { }) });
  /* the language menu in the header; a new language redraws the view in place, keeping what was chosen */
  I18N.picker($('#langSel'), $('#langName'));
  addEventListener('lastbook:lang', () => { if (S) render(false) });

  I18N.ready.then(load).then(() => { if (view === 'job') poll() });
})();
