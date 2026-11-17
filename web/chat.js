'use strict';
/* "Ask the library" — chat with the local model, grounded in the library. Conversations stay on this device only.
   Words: t() and th() from web/i18n.js, keys chat.* in web/locales/. Tables below hold keys, translated when shown. */
const Chat = (() => {
  const STARTERS = [
    ['water', 'chat.starter.water'],
    ['hurt', 'chat.starter.hurt'],
    ['power', 'chat.starter.power'],
    ['food', 'chat.starter.food'],
    ['sanitation', 'chat.starter.sanitation'],
    ['comms', 'chat.starter.comms'],
    ['grow', 'chat.starter.grow'],
    ['repair', 'chat.starter.repair'],
  ];
  const ABOUT_STARTERS = { doc: ['chat.about.doc_simple', 'chat.about.doc_warnings', 'chat.about.doc_video'],
    video: ['chat.about.video_summary', 'chat.about.video_safety', 'chat.about.video_manual'],
    topic: ['chat.about.topic_first', 'chat.about.topic_warnings', 'chat.about.topic_video'] };
  const STATES = { starting: 'chat.state.starting', thinking: 'chat.state.thinking', searching: 'chat.state.searching', reading: 'chat.state.reading', writing: 'chat.state.writing' };
  // Where an answer is: search, then read, then write (none yet while the local AI starts). A later round can search again, so a step can light up a second time.
  const STAGES = ['chat.stage.search', 'chat.stage.read', 'chat.stage.write'], STAGE_AT = { starting: -1, thinking: 0, searching: 0, reading: 1, writing: 2 };
  // the kinds search_library may be limited to (the engine's assistant, crates/engine/src/ai/assistant.rs); another kind shows as the model sent it
  const KIND = { manual: 'chat.kind.manual', video: 'chat.kind.video' };
  function progress(m) {
    const at = STAGE_AT[m.state] ?? 0, pct = m.state === 'starting' && m.pct ? Math.max(0, Math.min(100, +m.pct || 0)) : null;
    const stages = STAGES.map((k, i) => `<li class="${i < at ? 'done' : i === at ? 'now' : ''}">${th(k)}</li>`).join('');
    return `<ol class="wk-stages" aria-hidden="true">${stages}</ol><span class="wk-bar${pct === null ? ' run' : ''}" aria-hidden="true"><span${pct === null ? '' : ` style="width:${pct}%"`}></span></span>`;
  }
  let chats = load(), live = {};   // live[chatId] = {abort, state, steps}
  let status = null;

  function load() { try { return JSON.parse(localStorage.getItem('sg.chats')) || [] } catch { return [] } }
  const keep = () => { try { return localStorage.getItem('sg.chatKeep') !== 'no' } catch { return true } };
  function persist() { if (!keep()) return; try { localStorage.setItem('sg.chats', JSON.stringify(chats.slice(0, 60))) } catch { } }
  const byId = id => chats.find(c => c.id === id);
  const newId = () => Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
  const count = () => chats.length;

  async function getStatus() {
    try { status = await (await fetch('/api/assistant/status')).json() } catch { status = { online: false, error: 'server' } }
    return status;
  }

  /* ---------------------------------------------------------------- page -- */
  let root = null, cur = null, aboutCtx = null;
  async function page(main, parts, q) {
    root = main;
    const id = parts[0];
    cur = id ? byId(id) : null;
    if (id && !cur) { swap('#/ask'); return }
    const about = q.get('about');
    aboutCtx = !cur && about ? await describe(about) : (cur && cur.about ? cur.aboutInfo : null);
    document.title = (cur ? cur.title : t('chat.title')) + ' · Lastbook';
    $('#tbTitle').textContent = cur ? cur.title : t('chat.title');
    main.innerHTML = `<div class="chat">
      <aside class="chat-side" id="chatSide" aria-label="${th('chat.side.label')}">
        <div class="cs-head"><b>${th('chat.side.title')}</b><button class="iconbtn" id="closeSide" aria-label="${th('chat.side.close')}">${ico('back')}</button></div>
        <a class="btn plain" href="#/ask" id="newChat2">${ico('ask')}${th('chat.new_chat')}</a>
        <ul class="cs-list" id="csList"></ul>
        <div class="cs-privacy">
          <label><input type="checkbox" id="keepChats" ${keep() ? 'checked' : ''}> ${th('chat.keep')}</label>
          <p class="small muted">${th('chat.privacy')}</p>
          <button class="btn plain" id="clearChats">${th('chat.delete_all')}</button>
        </div>
      </aside>
      <section class="chat-main">
        <div class="chat-top">
          <button class="btn plain chat-histbtn" id="openSide" aria-label="${th('chat.side.show')}">${ico("clock")}<span>${th('chat.side.title')}</span></button>
          <div class="chat-status" id="chatStatus" aria-live="polite"></div>
          <a class="btn plain" href="#/ask" id="newChat" aria-label="${th('chat.new_chat')}">${ico('ask')}<span>${th('chat.new')}</span></a>
        </div>
        ${q.get('from') && !cur ? `<div style="margin:2px 0 8px">${backButton(q.get('from'))}</div>` : ''}
        <div class="ai-setup" id="aiSetup" hidden></div>
        <div class="chat-log" id="chatLog" role="log" aria-live="polite" aria-label="${th('chat.log_label')}"></div>
        <form class="composer" id="composer">
          <div class="ctxchip" id="ctxChip" hidden></div>
          <div class="comp-row">
            <label class="sr" for="chatIn">${th('chat.question_label')}</label>
            <textarea id="chatIn" rows="1" placeholder="${th('chat.placeholder')}" enterkeyhint="send"></textarea>
            <button class="sendbtn" id="sendBtn" aria-label="${th('chat.send')}">${ico("up")}</button>
          </div>
          <p class="comp-note">${th('chat.note')}</p>
        </form>
      </section></div>`;
    renderSide(); renderCtx(); renderLog();
    const ta = $('#chatIn');
    ta.addEventListener('input', autosize);
    ta.addEventListener('keydown', e => {
      if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); $('#composer').requestSubmit() }
      if (e.key === 'Escape') stop();
    });
    $('#composer').addEventListener('submit', e => { e.preventDefault(); if (busy()) return stop(); send(ta.value) });
    $('#openSide').onclick = () => $('#chatSide').classList.add('open');
    $('#closeSide').onclick = () => $('#chatSide').classList.remove('open');
    $('#keepChats').onchange = e => {
      try { localStorage.setItem('sg.chatKeep', e.target.checked ? 'yes' : 'no') } catch { }
      if (!e.target.checked) { try { localStorage.removeItem('sg.chats') } catch { } toast(t('chat.toast.forget')) } else { persist(); toast(t('chat.toast.keep')) }
    };
    $('#clearChats').onclick = () => { if (!confirm(t('chat.confirm_delete_all'))) return; Object.values(live).forEach(l => l.abort && l.abort()); chats = []; try { localStorage.removeItem('sg.chats') } catch { } location.hash = '#/ask' };
    leaving = () => { document.removeEventListener('keydown', escStop) };
    document.addEventListener('keydown', escStop);
    statusLine();
    const initial = q.get('q');
    if (initial && !cur) { history.replaceState(null, '', '#/ask'); send(initial) }
    else if (matchMedia('(hover:hover)').matches) setTimeout(() => ta.focus({ preventScroll: true }), 60);
    const y = scrolls[here()];
    if (y != null) requestAnimationFrame(() => scrollTo(0, y)); else if (cur && cur.messages.length) scrollEnd(true); else scrollTo(0, 0);   // a new chat opens at its top
  }
  const escStop = e => { if (e.key === 'Escape' && busy()) stop() };
  function autosize() { const ta = $('#chatIn'); if (!ta) return; ta.style.height = 'auto'; ta.style.height = Math.min(ta.scrollHeight, 180) + 'px' }
  const busy = () => cur && live[cur.id] && live[cur.id].running;
  // The optional local AI (desktop app): [ready?, the line to show, as plain text]. The server version has no s.local.
  // l.message is the engine's own sentence (English), shown as it comes.
  function localLine(l) {
    const gb = n => I18N.unit(Math.round(n / 1e9), 'gigabyte');
    return ({
      ready: () => [true, t('chat.local.ready')],
      stopped: () => [true, t('chat.local.stopped')],
      loading: () => [true, l.pct ? t('chat.local.loading_pct', { pct: l.pct }) : t('chat.local.loading')],
      checking: () => [false, l.pct != null ? t('chat.local.checking_pct', { pct: l.pct }) : t('chat.local.checking')],
      measuring: () => [false, t('chat.local.measuring')],
      absent: () => [false, t('chat.local.absent')],
      wrong: () => [false, l.message || t('chat.local.wrong')],
      too_big: () => [false, l.message || t('chat.local.too_big', { size: gb(l.needs_bytes || 0) })],
      off: () => [false, t('chat.local.off')],
      error: () => [false, l.message || t('chat.local.error')],
    }[l.state] || (() => [false, l.message || t('chat.local.unavailable')]))();
  }
  let polling = null;
  async function statusLine() {
    const s = await getStatus(), el = $('#chatStatus'); if (!el) return;
    const ix = s.index || {}, l = s.local;
    const still = ix.state === 'indexing' || ix.state === 'indexing (other process)' ? ` · <span class="muted">${th('chat.status.indexing', { n: ix.chunks || 0 })}</span>` : '';
    if (l && (/ready|stopped|loading/.test(l.state) || !s.online)) {
      const [ok, line] = localLine(l);
      el.innerHTML = `<span class="${ok ? 'dotok' : 'dotoff'}"></span><span>${esc(line)}${ok ? still : `<span class="long"> — ${th('chat.status.still_work')}</span>`}</span>`;
    } else {
      el.innerHTML = s.online ? `<span class="dotok"></span>${th('chat.status.ready')}${still}`
        : `<span class="dotoff"></span><span>${th('chat.status.offline')}<span class="long"> — ${th('chat.status.still_browse')}</span></span>`;
    }
    const sb = $('#sendBtn'); if (sb) sb.title = s.online ? t('chat.send') : t('chat.send_offline');
    setupCard(s);
    clearTimeout(polling);
    if (l && /checking|measuring|loading/.test(l.state) && $('#chatStatus')) polling = setTimeout(statusLine, 1000);
  }
  // How to add the local AI, on a new chat when it is missing, wrong or off. The file comes from the website, outside the app.
  function setupCard(s) {
    const box = $('#aiSetup'), l = s.local; if (!box) return;
    const show = !!l && !s.online && /absent|wrong|off|error/.test(l.state) && !(cur && cur.messages.length);
    box.hidden = !show;
    if (!show) return;
    box.innerHTML = l.state === 'off'
      ? `<p>${th('chat.setup.off')}</p><div class="row"><button class="btn" data-ai="on">${th('chat.setup.turn_on')}</button></div>`
      : l.state === 'error' ? `<p>${esc(l.message || t('chat.local.error'))}</p><div class="row"><button class="btn" data-ai="rescan">${th('chat.setup.look_again')}</button></div>`
      : `<p><b>${th('chat.setup.add_title')}</b> ${th('chat.setup.add_size', { size: I18N.unit(17, 'gigabyte') })}</p>
         <div class="row"><a class="btn" href="/ui/setup.html#manage">${th('chat.setup.add')}</a><button class="btn plain" data-ai="rescan">${th('chat.setup.look_again')}</button></div>
         <p class="small">${th('chat.setup.have_file', { file: I18N.raw(`<code>${esc(l.file)}</code>`), folder: I18N.raw('<b>LastbookData</b>'), again: t('chat.setup.look_again') })}</p>`;
    $$('[data-ai]', box).forEach(b => b.onclick = async () => {
      b.disabled = true;
      try { await fetch('/api/assistant/local', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ action: b.dataset.ai }) }) } catch { }
      setTimeout(statusLine, 300);
    });
  }
  async function describe(about) {
    const [kind, id, n] = about.split(':');
    try {
      if (kind === 'topic') { const topic = (await topics()).find(x => x.id === id); return { about, label: topic ? (SHORT[id] ? t(SHORT[id]) : topic.title) : id, sub: t('chat.ctx.guide'), kind } }
      const d = await api('/api/doc/' + encodeURIComponent(id));
      return { about, kind, label: d.title, sub: kind === 'video' ? (n ? t('chat.ctx.video_at', { time: clock(+n) }) : t('chat.ctx.video')) : t('chat.ctx.page', { page: n || '1' }), href: kind === 'video' ? `#/watch/${encodeURIComponent(id)}${n ? '?t=' + n : ''}` : `#/read/${encodeURIComponent(id)}?p=${n || 1}` };
    } catch { return null }
  }
  function renderCtx() {
    const el = $('#ctxChip'); if (!el) return;
    const a = aboutCtx;
    if (!a) { el.hidden = true; return }
    el.hidden = false;
    el.innerHTML = `${ico(a.kind === 'video' ? 'video' : a.kind === 'topic' ? 'book' : 'page')}<span>${th('chat.ctx.asking', { label: I18N.raw(`<b>${esc(a.label)}</b>`) })} <span class="muted">${esc(a.sub || '')}</span></span>${cur && cur.messages.length ? '' : `<button type="button" aria-label="${th('chat.ctx.remove')}" id="ctxX">×</button>`}`;
    const x = $('#ctxX'); if (x) x.onclick = () => { aboutCtx = null; renderCtx(); renderLog() };
  }
  function renderSide() {
    const el = $('#csList'); if (!el) return;
    el.innerHTML = chats.length ? chats.map(c => `<li><a href="#/ask/${c.id}" ${cur && cur.id === c.id ? 'aria-current="page"' : ''}><b>${esc(c.title)}</b><small>${esc(I18N.date(c.updated, { month: 'short', day: 'numeric' }))} · ${th('chat.side.questions', { n: c.messages.filter(m => m.role === 'user').length })}</small></a><button class="iconbtn" data-del="${c.id}" aria-label="${th('chat.side.delete')}">×</button></li>`).join('')
      : `<li class="muted small" style="padding:8px 4px">${th('chat.side.empty')}</li>`;
    const clr = $('#clearChats'); if (clr) clr.hidden = !chats.length;   // nothing to delete
    $$('[data-del]', el).forEach(b => b.onclick = () => { const id = b.dataset.del; live[id]?.abort?.(); chats = chats.filter(c => c.id !== id); persist(); if (cur && cur.id === id) location.hash = '#/ask'; else renderSide() });
    // the list fades at an edge with more chats past it, so a row cut off there reads as more to scroll to
    const edge = () => { const more = el.scrollHeight - el.clientHeight; el.classList.toggle('more-b', more > 1 && el.scrollTop < more - 1); el.classList.toggle('more-t', more > 1 && el.scrollTop > 1) };
    el.onscroll = edge; edge();
    if (!el.lbEdge) { el.lbEdge = new ResizeObserver(edge); el.lbEdge.observe(el) }
  }

  /* --------------------------------------------------------------- log --- */
  function renderLog() {
    const log = $('#chatLog'); if (!log) return;
    if (!cur || !cur.messages.length) {
      const st = aboutCtx ? ABOUT_STARTERS[aboutCtx.kind] || [] : null;
      log.innerHTML = `<div class="chat-empty">
        <div class="ce-mark">${ico('ask')}</div>
        <h1>${th('chat.title')}</h1>
        <p class="lead">${th('chat.empty.lead')}</p>
        <div class="starters">${(st ? st.map(k => ['ask', k]) : STARTERS).map(([i, k]) => `<button class="starter" ${tcStyle(i)} data-start="${th(k)}"><span class="ico">${ico(i)}</span><span>${th(k)}</span></button>`).join('')}</div>
        <p class="small muted" style="margin-top:18px">${th('chat.empty.note')}</p></div>`;
      $$('[data-start]', log).forEach(b => b.onclick = () => send(b.dataset.start));
      fitEmpty();
      return;
    }
    log.innerHTML = cur.messages.map((m, i) => m.role === 'user' ? `<div class="msg user"><div class="bubble">${esc(m.content)}</div></div>` : `<div class="msg bot" id="m${i}">${botHtml(m, i)}</div>`).join('');
    wire(log);
  }
  // The page itself never scrolls while a chat is empty: in a window too short for every suggestion (or with lines wrapped narrow beside
  // the chats column), the last ones go, a row at a time down to one row, then the note under them, the introduction and the mark.
  function fitEmpty() {
    const e = $('.chat-empty'); if (!e) return;
    const all = $$('.starter', e), extra = ['p.small', '.lead', '.ce-mark'].map(s => $(s, e)).filter(Boolean);
    [...all, ...extra].forEach(x => x.style.removeProperty('display'));
    const over = () => document.scrollingElement.scrollHeight > innerHeight;
    if (!over()) return;
    const shown = all.filter(x => x.offsetWidth);
    for (let k = shown.length; k > 2 && over(); k -= 2) shown.slice(k - 2, k).forEach(x => x.style.display = 'none');
    for (const x of extra) if (over()) x.style.display = 'none';
  }
  addEventListener('resize', () => { if ($('.chat-empty')) fitEmpty() });
  function updateBot(i) {
    const el = $('#m' + i); if (!el || !cur) return renderLog();
    el.innerHTML = botHtml(cur.messages[i], i); wire(el);
  }
  function botHtml(m, i) {
    const l = live[cur.id], running = l && l.running && i === cur.messages.length - 1;
    const steps = (m.steps || []).map(s => `<li>${stepText(s)}</li>`).join('');
    const found = m.sources || [];
    let h = '';
    if (running) h += `<div class="working"><b>${th(STATES[m.state] || 'chat.state.working')}${m.state === 'starting' && m.pct ? ' ' + th('chat.pct', { pct: m.pct }) : ''}</b>${progress(m)}${steps ? `<ul class="steps">${steps}</ul>` : ''}</div>`;
    if (m.error) h += `<div class="ans-error">${ico('warn')}<div><b>${esc(m.error)}</b>${m.offline ? `<p>${th('chat.offline_pages')}</p>` : ''}</div></div>`;
    if (m.content) h += `<div class="answer">${md(m.content, m)}</div>`;
    if (running && found.length) h += `<div class="prefound"><p class="small muted">${th('chat.prefound')}</p>${srcCards(found.slice(0, 6), true)}</div>`;
    if (m.fallback) h += srcCards(m.fallback, true);
    if (!running && m.content) {
      const cited = m.cited || [];
      if ((m.invalid || []).length) h += `<p class="small" style="color:var(--stop)">${th('chat.invalid', { list: m.invalid.join(', ') })}</p>`;
      h += `<div class="legend small"><span><span class="lg-c">1</span> ${th('chat.legend.linked')}</span><span><span class="lg-u">${th('chat.legend.dotted')}</span> ${th('chat.legend.own')}</span></div>`;
      if (cited.length) h += `<h3 class="srch">${th('chat.sources', { n: cited.length })}</h3>${srcCards(cited, false)}`;
      else h += `<p class="notice small">${th('chat.no_sources')}</p>`;
      const other = (m.consulted || []).filter(s => s.kind !== 'place');
      if (other.length) h += `<details class="also"><summary>${th('chat.also', { n: other.length })}</summary>${srcCards(other.slice(0, 10), true)}</details>`;
      if ((m.steps || []).length) h += `<details class="also"><summary>${m.latency ? th('chat.how_time', { n: m.steps.length, secs: m.latency.total_s }) : th('chat.how', { n: m.steps.length })}</summary><ul class="steps">${steps}</ul></details>`;
      h += `<div class="ans-actions"><button class="btn plain" data-act="copy" data-i="${i}">${th('chat.act.copy')}</button><button class="btn plain" data-act="save" data-i="${i}">${ico('save')}${th('chat.act.save')}</button><button class="btn plain" data-act="export">${th('chat.act.export')}</button>${i === cur.messages.length - 1 ? `<button class="btn plain" data-act="retry">${th('chat.act.retry')}</button>` : ''}</div>`;
    }
    return h;
  }
  function stepText(s) {
    // s.args is the model's own tool call, as it sent it: use only plain text and numbers from it, never convert anything else.
    const plain = v => typeof v === 'string' || typeof v === 'number' ? v : '';
    const raw = s.args && typeof s.args === 'object' ? s.args : {}, r = s.summary || {};
    const a = { query: plain(raw.query), kind: plain(raw.kind), doc_id: plain(raw.doc_id), page: plain(raw.page), pages: raw.pages };
    if (s.name === 'search_library') {
      const kind = a.kind && a.kind !== 'any' ? (KIND[a.kind] ? t(KIND[a.kind]) : a.kind) : '';
      return `${kind ? th('chat.step.searched_kind', { query: a.query || '', kind }) : th('chat.step.searched', { query: a.query || '' })} — ${r.count == null ? '…' : th('chat.step.passages', { n: r.count })}`;
    }
    if (s.name === 'read_pages') {
      const pages = (Array.isArray(a.pages) ? a.pages : [a.pages]).filter(p => typeof p === 'number' || typeof p === 'string');
      return r.error ? th('chat.step.read_unknown') : th('chat.step.read', { n: pages.length, title: r.title || a.doc_id, pages: pages.join(', ') });
    }
    if (s.name === 'view_page') return r.error ? th('chat.step.view_failed') : th('chat.step.viewed', { title: r.title || '', page: String(a.page) });
    if (s.name === 'get_timed_transcript') return r.error ? (r.title ? th('chat.step.no_transcript', { title: r.title }) : th('chat.step.no_transcript_video')) : th('chat.step.transcript', { title: r.title || '' });
    if (s.name === 'lookup_place') return th('chat.step.place', { query: a.query || '' });
    if (s.name === 'found') return th('chat.step.found', { n: s.count });
    return esc(s.name);
  }
  function srcHref(s) {
    if (s.kind === 'place') return `#/map?lat=${s.lat}&lon=${s.lon}&z=16&pin=${encodeURIComponent(s.title || s.name || t('chat.place_from_answer'))}`;
    if (s.kind === 'video') return `#/watch/${encodeURIComponent(s.doc_id)}?${s.t != null ? 't=' + Math.floor(s.t) + '&' : ''}from=${encodeURIComponent(here())}`;
    return `#/read/${encodeURIComponent(s.doc_id)}?${s.page ? 'p=' + s.page + '&' : ''}from=${encodeURIComponent(here())}`;
  }
  function srcCards(list, compact) {
    return `<div class="srccards ${compact ? 'compact' : ''}">${list.map(s => {
      let pv;
      if (s.kind === 'video') pv = `<span class="pv v">${s.thumb ? `<img loading="lazy" alt="" src="${fileUrl(s.thumb)}">` : ''}<span class="pg">${s.t != null ? clock(s.t) : th('chat.src.video')}</span></span>`;
      else if (s.kind === 'pdf' && s.page) pv = `<span class="pv"><img loading="lazy" alt="" src="/api/page/${encodeURIComponent(s.doc_id)}/${s.page}.png?w=300" onerror="this.remove()"><span class="pg">${th('chat.src.p', { page: String(s.page) })}</span></span>`;
      else pv = `<span class="pv w">${ico(s.kind === 'place' ? 'pin' : 'page')}</span>`;
      const where = s.kind === 'video' ? (s.t != null ? th('chat.src.at', { time: clock(s.t) }) : '') : s.page ? (s.pages ? th('chat.src.page_of', { page: String(s.page), pages: String(s.pages) }) : th('chat.src.page', { page: String(s.page) })) : '';
      return `<a class="srccard" href="${srcHref(s)}" id="src-${esc(s.key || '')}">${pv}<span class="sc-body">${s.key ? `<span class="sc-key">${esc(s.key.replace(/^S/, ''))}</span>` : ''}<b>${esc(s.title)}</b>
        <small>${esc(s.publisher || '')}${s.year ? ' · ' + esc(s.year) : ''}${where ? ' · ' + where : ''}${s.trust ? ' · ' + esc(trustOf(s)) : ''}</small>
        ${!compact && s.excerpt ? `<span class="sc-ex">${th('chat.src.excerpt', { text: s.excerpt.slice(0, 220) + (s.excerpt.length > 220 ? '…' : '') })}</span>` : ''}
        ${s.image ? `<span class="sc-tag">${th('chat.src.diagram')}</span>` : ''}</span></a>`;
    }).join('')}</div>`;
  }

  /* markdown-lite with citation chips; sentences without a citation are marked as the model's own wording.
     The three section headings are found by the model's English words (the assistant's prompt, crates/engine/src/ai/); this page names them in the interface's language. */
  function md(text, m) {
    const keys = new Set((m.cited || m.sources || []).map(s => s.key));
    const cite = k => keys.has(k) || !m.cited ? `<a class="cite" href="#src-${k}" data-key="${k}" title="${th('chat.cite', { key: k })}">${k.replace(/^S/, '')}</a>` : '';
    const inline = s => esc(s).replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>').replace(/(^|\W)\*(?!\s)(.+?)\*(?=\W|$)/g, '$1<em>$2</em>')
      .replace(/\[([SM]\d+)\]/g, (_, k) => cite(k));
    const lines = text.replace(/\r/g, '').split('\n');
    let out = '', list = null, section = 'main';
    const close = () => { if (list) { out += `</${list}>`; list = null } };
    const mark = raw => {  // wrap sentences that carry no citation (main sections only)
      if (section !== 'main' || m.running) return inline(raw);
      // a sentence starts with a capital in any script (É, Ł, Ü), or with ¿ ¡ « „ as Spanish, French and German write them
      return raw.split(/(?<=[.!?…])\s+(?=[\p{Lu}(“"«„¿¡])/u).map(sn => /\[[SM]\d+\]/.test(sn) || sn.replace(/\*|_/g, '').trim().length < 28 || /:$/.test(sn.trim()) ? inline(sn) : `<span class="uncited" title="${th('chat.uncited')}">${inline(sn)}</span>`).join(' ');
    };
    for (let ln of lines) {
      const row = ln.trim();
      if (!row) { close(); continue }
      const head = /^#{1,4}\s+(.*)$/.exec(row) || /^\*\*([^*]+:)\*\*$/.exec(row) || /^\*\*([^*]+)\*\*:?$/.exec(row);
      const hText = head ? head[1] : '';
      const sec = /not found in the library/i.test(row) ? 'missing' : /sources disagree/i.test(row) ? 'disagree' : /own reasoning/i.test(row) ? 'reasoning' : null;
      if (sec || head) {
        close();
        if (out.includes('<div class="sec-')) out += '</div>';
        section = sec || (head ? 'main' : section);
        if (sec) { out += `<div class="sec-${sec}"><h4>${sec === 'missing' ? th('chat.sec.missing') : sec === 'disagree' ? th('chat.sec.disagree') : th('chat.sec.reasoning')}</h4>`; const rest = row.replace(/^[#*\s]*(not found in the library|sources disagree|my own reasoning \(not from the library\)|my own reasoning)[:*\s]*/i, ''); if (rest) out += `<p>${inline(rest)}</p>`; continue }
        out += `<h4>${inline(hText.replace(/:$/, ''))}</h4>`; continue;
      }
      const ul = /^[-*•]\s+(.*)$/.exec(row), ol = /^(\d+)[.)]\s+(.*)$/.exec(row);
      if (ul || ol) {
        const want = ul ? 'ul' : 'ol';
        if (list !== want) { close(); out += `<${want}>`; list = want }
        out += `<li>${mark(ul ? ul[1] : ol[2])}</li>`; continue;
      }
      close(); out += `<p>${mark(row)}</p>`;
    }
    close();
    if (out.includes('<div class="sec-')) out += '</div>';
    return out;
  }

  function wire(el) {
    $$('[data-act]', el).forEach(b => b.onclick = () => {
      const i = +b.dataset.i, m = cur.messages[i];
      if (b.dataset.act === 'copy') { navigator.clipboard?.writeText(plain(m)).then(() => toast(t('chat.toast.copied')), () => toast(t('chat.toast.copy_failed'))) }
      if (b.dataset.act === 'save') {
        const q = cur.messages[i - 1]?.content || cur.title;
        toggleSave({ key: `a:${cur.id}:${i}`, title: t('chat.saved_title', { q: q.slice(0, 80) }), href: '#/ask/' + cur.id, sub: t('chat.saved_sub', { n: (m.cited || []).length, date: I18N.date(new Date()) }) }, null);
      }
      if (b.dataset.act === 'export') exportChat(cur);
      if (b.dataset.act === 'retry') { const q = cur.messages[cur.messages.length - 2]?.content; cur.messages.splice(-2, 2); send(q) }
    });
    // citation chips scroll to their card instead of changing the route
    $$('a.cite', el).forEach(a => a.onclick = e => { e.preventDefault(); const c = $('#src-' + a.dataset.key, el.closest('.msg') || el); if (c) { c.scrollIntoView({ block: 'center', behavior: 'smooth' }); c.classList.add('flash'); setTimeout(() => c.classList.remove('flash'), 1200) } });
  }
  function plain(m) {
    const map = Object.fromEntries((m.cited || []).map(s => [s.key, s]));
    let text = m.content.replace(/\[([SM]\d+)\]/g, (_, k) => map[k] ? `[${k.replace(/^S/, '')}]` : '');
    if ((m.cited || []).length) text += '\n\n' + t('chat.export.sources') + '\n' + m.cited.map(s => `[${s.key.replace(/^S/, '')}] ${s.title} — ${s.publisher || ''}${s.year ? ', ' + s.year : ''}${s.page ? ', ' + t('chat.src.page', { page: String(s.page) }) : ''}${s.t != null ? ', ' + t('chat.src.at', { time: clock(s.t) }) : ''}  ${location.origin}/${srcHref(s).replace(/&?from=[^&]*/, '')}`).join('\n');
    return text;
  }
  function exportChat(c) {
    let md = `# ${c.title}\n\n${t('chat.export.from', { date: I18N.date(new Date(), { dateStyle: 'medium', timeStyle: 'short' }) })}\n${t('chat.export.note')}\n`;
    for (const m of c.messages) md += m.role === 'user' ? `\n## ${t('chat.export.question')}\n\n${m.content}\n` : `\n## ${t('chat.export.answer')}\n\n${m.content ? plain(m) : t('chat.export.no_answer', { why: m.error || t('chat.export.stopped') })}\n`;
    const a = document.createElement('a');
    a.href = URL.createObjectURL(new Blob([md], { type: 'text/markdown' }));
    a.download = `survival-chat-${new Date().toISOString().slice(0, 10)}.md`;
    document.body.append(a); a.click(); setTimeout(() => { URL.revokeObjectURL(a.href); a.remove() }, 500);
    toast(t('chat.toast.exported'));
  }
  // Earlier answers' [S#] keys only mean something inside their own turn; give the model readable references instead.
  function history_(c) {
    return c.messages.filter(m => m.content).map(m => {
      if (m.role !== 'assistant') return { role: m.role, content: m.content };
      const map = Object.fromEntries((m.cited || []).map(s => [s.key, s]));
      return { role: 'assistant', content: m.content.replace(/\[([SM]\d+)\]/g, (_, k) => map[k] ? `(${map[k].title}${map[k].page ? ', p. ' + map[k].page : ''}${map[k].t != null ? ', ' + clock(map[k].t) : ''})` : '') };
    });
  }

  /* -------------------------------------------------------------- send --- */
  async function send(text) {
    text = (text || '').trim();
    if (!text) return;
    if (!cur) {
      cur = { id: newId(), title: text.slice(0, 70), created: Date.now(), updated: Date.now(), messages: [], about: aboutCtx ? aboutCtx.about : null, aboutInfo: aboutCtx };
      chats.unshift(cur);
      history.replaceState(null, '', '#/ask/' + cur.id);
    }
    const c = cur;
    c.messages.push({ role: 'user', content: text });
    const m = { role: 'assistant', content: '', state: 'thinking', steps: [], sources: [], running: true };
    c.messages.push(m);
    c.updated = Date.now(); persist();
    const ta = $('#chatIn'); if (ta) { ta.value = ''; autosize() }
    renderSide(); renderCtx(); renderLog(); setBusy(true); scrollEnd(true);
    const idx = c.messages.length - 1, ctl = new AbortController();
    live[c.id] = { running: true, abort: () => ctl.abort() };
    const redraw = throttle(() => { if (cur === c) { updateBot(idx); scrollEnd() } }, 80);
    try {
      const st = await getStatus();
      if (!st.online) throw Object.assign(new Error('offline'), { offline: true, why: st.local && localLine(st.local)[1] });
      const r = await fetch('/api/chat', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ messages: history_(c).slice(0, -1).concat([{ role: 'user', content: text }]), about: c.about }), signal: ctl.signal });
      if (!r.ok || !r.body) throw new Error(t('chat.err.refused', { status: String(r.status) }));
      const rd = r.body.getReader(), dec = new TextDecoder();
      let buf = '';
      for (;;) {
        const { value, done } = await rd.read();
        if (done) break;
        buf += dec.decode(value, { stream: true });
        let k;
        while ((k = buf.indexOf('\n\n')) >= 0) {
          const chunk = buf.slice(0, k); buf = buf.slice(k + 2);
          if (!chunk.startsWith('data:')) continue;
          let ev; try { ev = JSON.parse(chunk.slice(5)) } catch { continue }
          onEvent(m, ev);
          redraw();
        }
      }
      if (m.running && !m.content && !m.error) m.error = t('chat.err.stopped_unexpectedly');
    } catch (e) {
      if (e.name === 'AbortError') { m.stopped = true; if (!m.content) m.error = t('chat.err.stopped') }
      else if (e.offline || /model_unavailable/.test(e.message)) { m.offline = true; m.error = e.why || t('chat.err.model_off'); await fallback(m, text) }
      else m.error = e.message || t('chat.err.failed');
    }
    m.running = false; delete m.state;
    if (m.stopped && m.content) m.content += '\n\n' + t('chat.stopped_note');
    live[c.id].running = false;
    c.updated = Date.now(); persist();
    if (cur === c) { updateBot(idx); setBusy(false); scrollEnd(); const box = $('#chatIn'); if (box && matchMedia('(hover:hover)').matches) box.focus({ preventScroll: true }) }
  }
  function onEvent(m, ev) {
    if (ev.type === 'state') { m.state = ev.state; m.pct = ev.pct }
    else if (ev.type === 'found') m.steps.push({ name: 'found', count: ev.count });
    else if (ev.type === 'tool') { if (ev.phase === 'start') m.steps.push({ name: ev.name, args: ev.args }); else { const s = [...m.steps].reverse().find(x => x.name === ev.name && !x.summary); if (s) s.summary = ev.summary } m.state = 'searching' }
    else if (ev.type === 'sources') m.sources = ev.sources;
    else if (ev.type === 'delta') m.content += ev.text;
    else if (ev.type === 'reset') m.content = '';
    else if (ev.type === 'done') { m.content = ev.answer; m.cited = ev.sources; m.consulted = ev.consulted; m.invalid = ev.invalid; m.latency = ev.latency; m.content = m.content.replace(/\[([SM]\d+)\]/g, (x, k) => (ev.invalid || []).includes(k) ? '' : x) }
    else if (ev.type === 'error') {   // ev.text is the engine's own sentence
      if (ev.error === 'model_unavailable') { m.offline = true; m.error = ev.text; fallback(m, (cur.messages[cur.messages.length - 2] || {}).content || '') }
      else { m.error = ev.text; if (ev.sources) m.fallback = ev.sources.slice(0, 8) }
    }
    else if (ev.type === 'stopped') m.stopped = true;
  }
  async function fallback(m, q) {
    try { const d = await api('/api/retrieve?q=' + encodeURIComponent(q) + '&k=8&lang=' + I18N.lang); m.fallback = d.results.map((r, i) => ({ ...r, key: '' })); if (!m.fallback.length) m.error += ' ' + t('chat.err.nothing_found') }
    catch { m.fallback = [] }
  }
  function stop() { if (cur && live[cur.id]) live[cur.id].abort() }
  function setBusy(on) {
    const b = $('#sendBtn'); if (!b) return;
    b.innerHTML = on ? '<span class="stopsq"></span>' : ico('up');
    b.setAttribute('aria-label', on ? t('chat.stop') : t('chat.send'));
    b.classList.toggle('stop', on);
  }
  function scrollEnd(force) {
    const near = innerHeight + scrollY > document.body.scrollHeight - 260;
    if (force || near) requestAnimationFrame(() => scrollTo(0, document.body.scrollHeight));
  }
  function throttle(f, ms) { let last = 0, p = null; return () => { const n = Date.now(); if (n - last > ms) { last = n; f() } else { clearTimeout(p); p = setTimeout(() => { last = Date.now(); f() }, ms) } } }

  return { page, count };
})();
