'use strict';

const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const $ = (sel, el = document) => el.querySelector(sel);

let S = null;          // последнее состояние от программы
let L = {};            // строки интерфейса на текущем языке
let tab = 'in';
let view = 'main';     // main | settings | about | onboarding
let invite = null;     // показанный код приглашения
let joinOpen = false;
let cloudTab = 'yandex';
let cloudFolders = null;
const onb = { step: 0, name: '', folder: '', shortcut: true, pin: true, mode: null, code: '', joined: null, busy: false };

// ---------- Языки и форматирование ----------

function esc(s) {
  return String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

// Строка по ключу; значения подстановок экранируются (сами строки — наши, в них бывает <b>).
function t(key, p) {
  const s = typeof L[key] === 'string' ? L[key] : key;
  return p ? s.replace(/\{(\w+)\}/g, (m, k) => (k in p ? esc(p[k]) : m)) : s;
}

function pluralForm(n) {
  const lang = L._lang;
  if (lang === 'ru' || lang === 'uk') {
    const a = n % 10, b = n % 100;
    if (a === 1 && b !== 11) return 'one';
    if (a >= 2 && a <= 4 && (b < 12 || b > 14)) return 'few';
    return 'many';
  }
  if (lang === 'fr' || lang === 'pt') return n <= 1 ? 'one' : 'other';
  if (lang === 'zh') return 'other';
  return n === 1 ? 'one' : 'other';
}

function tp(key, n) {
  const f = L[key];
  const s = f && typeof f === 'object' ? (f[pluralForm(n)] ?? f.other ?? f.many) : (f ?? key);
  return String(s).replace('{n}', n);
}

function fmtSize(b) {
  const u = (L.units || 'B|KB|MB|GB|TB').split('|');
  let v = b, i = 0;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  let num = i === 0 ? String(Math.round(v)) : v >= 100 ? v.toFixed(0) : v.toFixed(1).replace(/\.0$/, '');
  num = num.replace('.', L.decimal || '.');
  return `${num} ${u[i]}`;
}

function fmtEta(sec) {
  sec = Math.round(sec);
  if (sec < 60) return t('eta.s', { n: Math.max(sec, 1) });
  if (sec < 3600) return t('eta.m', { n: Math.round(sec / 60) });
  return t('eta.hm', { h: Math.floor(sec / 3600), m: Math.round((sec % 3600) / 60) });
}

function ago(ms) {
  const d = (Date.now() - ms) / 1000;
  if (d < 45) return t('ago.now');
  if (d < 3600) return t('ago.min', { n: Math.round(d / 60) });
  const date = new Date(ms), now = new Date();
  const time = date.toLocaleTimeString(L._lang, { hour: '2-digit', minute: '2-digit' });
  if (date.toDateString() === now.toDateString()) return t('ago.today', { time });
  const y = new Date(now); y.setDate(now.getDate() - 1);
  if (date.toDateString() === y.toDateString()) return t('ago.yesterday', { time });
  return date.toLocaleDateString(L._lang, { day: 'numeric', month: 'long' });
}

function q(name) { return t('quote', { name }); }

function toEl(html) {
  const tpl = document.createElement('template');
  tpl.innerHTML = html.trim();
  return tpl.content.firstElementChild;
}

function setHtml(el, html) {
  if (el._html !== html) { el.innerHTML = html; el._html = html; }
}

// Текст с абзацами: пустая строка — новый абзац, «• » — пункт списка.
function paragraphs(text) {
  return text.split('\n').filter(Boolean).map(l => (l.startsWith('• ') ? `<li>${esc(l.slice(2))}</li>` : `<p>${esc(l)}</p>`))
    .join('').replace(/(<li>.*?<\/li>)+/g, m => `<ul>${m}</ul>`);
}

function applyStatic() {
  document.documentElement.lang = L._lang;
  document.querySelectorAll('[data-t]').forEach(el => (el.textContent = t(el.dataset.t)));
  document.querySelectorAll('[data-t-title]').forEach(el => { el.title = t(el.dataset.tTitle); el.setAttribute('aria-label', el.title); });
}

// ---------- Сообщения и диалоги ----------

let toastTimer = null;
function toast(html, kind = 'ok') {
  const el = $('#toast');
  el.className = `toast ${kind}`;
  el.innerHTML = `${icon(kind === 'err' ? 'warning' : 'check')}<div>${html}</div>`;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.add('hidden'), kind === 'err' ? 7000 : 3000);
}

function confirmDialog(title, text, ok, danger = false) {
  return new Promise(resolve => {
    const m = $('#modal');
    m.innerHTML = `<div class="dialog"><div class="content"><h3>${title}</h3><p>${text}</p></div>
      <div class="buttons"><button class="btn ${danger ? 'danger' : 'primary'}" data-r="1">${ok}</button><button class="btn" data-r="0">${t('btn.cancel')}</button></div></div>`;
    m.classList.remove('hidden');
    m.onclick = e => {
      const b = e.target.closest('[data-r]');
      if (!b && e.target !== m) return;
      m.classList.add('hidden');
      resolve(b?.dataset.r === '1');
    };
  });
}

function legalDialog() {
  const m = $('#modal');
  m.innerHTML = `<div class="dialog tall"><div class="content legal">
      <h3>${t('about.terms_title')}</h3>${paragraphs(t('legal.terms'))}
      <h3>${t('about.privacy_title')}</h3>${paragraphs(t('legal.privacy'))}</div>
    <div class="buttons"><button class="btn primary" data-r="1">${t('btn.close')}</button></div></div>`;
  m.classList.remove('hidden');
  m.onclick = e => { if (e.target.closest('[data-r]') || e.target === m) m.classList.add('hidden'); };
}

async function run(promise, okText) {
  try {
    const r = await promise;
    if (okText) toast(okText);
    return r;
  } catch (e) {
    toast(esc(String(e)), 'err');
    throw e;
  }
}

// ---------- Главный экран ----------

function peerNote(p) {
  if (p.online && p.paused) return `${t('peer.online')} · ${t('peer.paused')}`;
  if (p.online) return `${t('peer.online')}${p.route ? ' · ' + esc(p.route) : ''}`;
  return p.last_seen ? `${t('peer.offline')} · ${ago(p.last_seen)}` : t('peer.offline');
}

function renderStatus() {
  const el = $('#status');
  if (!S.peers.length) {
    setHtml(el, `<div class="empty-peers">${icon('people', 'lg')}<div>${t('peer.none')}</div>
      <button class="btn sm primary" data-act="invite">${t('btn.invite')}</button></div>`);
    return;
  }
  setHtml(el, S.peers.slice(0, 4).map(p => `<div class="peer-row"><i class="peer-dot ${p.online ? 'on' : ''}"></i>
    <span class="peer-name">${esc(p.name)}</span><span class="peer-note">${peerNote(p)}</span></div>`).join('')
    + (S.peers.length > 4 ? `<div class="peer-row more" data-act="settings-family">${t('peer.more', { n: S.peers.length - 4 })}</div>` : ''));
}

function tile(it) {
  return `<div class="tile ${it.kind}">${icon(it.kind === 'folder' ? 'folder' : it.kind)}</div>`;
}

function sizeMeta(it) {
  const parts = [fmtSize(it.total)];
  if (it.is_folder) parts.push(tp('files', it.files));
  return parts;
}

function meta(parts) {
  return `<div class="meta">${parts.map(p => `<span>${p}</span>`).join('')}</div>`;
}

function progressBlock(cls = '') {
  return `<div class="progress ${cls}"><i data-role="bar"></i></div><div class="progress-text" data-role="ptext"></div>`;
}

function updateProgress(el, done, total, speed, extra) {
  const bar = el.querySelector('[data-role="bar"]');
  const text = el.querySelector('[data-role="ptext"]');
  if (!bar) return;
  const pct = total > 0 ? Math.min(100, (done / total) * 100) : 100;
  bar.style.width = `${pct}%`;
  const parts = [`${Math.floor(pct)}%`, t('progress.of', { done: fmtSize(done), total: fmtSize(total) })];
  if (speed > 1024) {
    parts.push(t('speed', { v: fmtSize(speed) }));
    if (done < total) parts.push(`~${fmtEta((total - done) / speed)}`);
  }
  if (extra) parts.push(extra);
  text.innerHTML = parts.join(' · ');
}

const dismissBtn = ids => `<button class="dismiss" title="${t('btn.dismiss')}" data-act="dismiss" data-id="${esc(ids)}">${icon('close')}</button>`;

function incomingCard(it) {
  const from = t('in.from', { name: it.peer });
  const when = ago(it.created_at);
  const id = esc(it.id);
  const name = `<div class="name" title="${esc(it.item)}">${esc(it.item)}</div>`;
  switch (it.state) {
    case 'new': {
      const badge = it.is_update
        ? `<span class="badge update">${icon('sync')}${t('in.badge_update')}</span>`
        : `<span class="badge">${icon('arrowDown')}${t('in.badge_new')}</span>`;
      const warn = it.has_exe ? `<div class="note warn">${icon('warning')}${t('in.exe_warn')}</div>` : '';
      return `<div class="card highlight">${tile(it)}<div class="body">${badge}${name}${meta([...sizeMeta(it), from, when])}${warn}
        <div class="actions"><button class="btn primary" data-act="accept" data-id="${id}">${icon('arrowDown')}${t('btn.accept')}</button>
        <button class="btn" data-act="decline" data-id="${id}">${t('btn.decline')}</button></div></div></div>`;
    }
    case 'queued':
      return `<div class="card">${tile(it)}<div class="body">${name}${meta([...sizeMeta(it), from])}
        <div class="state wait">${icon('sync')}${esc(it.message || t('in.queued'))}</div>
        <div class="actions"><button class="btn sm" data-act="decline" data-id="${id}">${t('btn.cancel')}</button></div></div></div>`;
    case 'downloading':
      return `<div class="card">${tile(it)}<div class="body">${name}${meta([from])}${progressBlock()}
        <div class="actions"><button class="btn sm" data-act="decline" data-id="${id}">${t('btn.cancel')}</button></div></div></div>`;
    case 'failed':
      return `<div class="card">${tile(it)}<div class="body">${name}${meta([...sizeMeta(it), from])}
        <div class="note err">${icon('warning')}${esc(it.message || t('in.failed'))}</div>
        <div class="actions"><button class="btn sm primary" data-act="accept" data-id="${id}">${t('btn.retry')}</button>
        <button class="btn sm" data-act="decline" data-id="${id}">${t('btn.decline')}</button></div></div></div>`;
    case 'done': {
      const conflict = it.conflicts.length
        ? `<div class="note">${icon('warning')}${t('in.conflict', { name: it.conflicts[0].split('/').pop() })}</div>` : '';
      const auto = it.auto ? `<span>${t('in.auto')}</span>` : '';
      return `<div class="card compact">${tile(it)}<div class="body">${name}
        <div class="meta"><span class="state ok">${icon('check')}${t('in.received')}</span><span>${fmtSize(it.total)}</span><span>${from}</span><span>${ago(it.updated_at)}</span>${auto}</div>
        ${conflict}<div class="actions"><button class="btn sm" data-act="open" data-path="${esc(it.path || '')}">${icon('folderOpen')}${t('btn.show')}</button></div></div>${dismissBtn(it.id)}</div>`;
    }
    case 'declined':
      return `<div class="card compact">${tile(it)}<div class="body">${name}
        <div class="meta"><span>${t('in.declined')}</span><span>${fmtSize(it.total)}</span><span>${from}</span></div></div>${dismissBtn(it.id)}</div>`;
    default:
      return `<div class="card compact">${tile(it)}<div class="body">${name}
        <div class="meta"><span class="state bad">${icon('warning')}${esc(it.message || t('in.gone'))}</span></div></div>${dismissBtn(it.id)}</div>`;
  }
}

const FINAL = ['delivered', 'declined', 'unavailable'];

function outStatus(it, short) {
  const online = S.peers.find(p => p.id === it.peer_id)?.online;
  switch (it.state) {
    case 'pending': return online ? t('out.sending') : short ? t('peer.offline') : t('out.waiting_offline', { name: it.peer });
    case 'offered': return t('out.offered');
    case 'accepted': return t('out.accepted');
    case 'downloading': return `${Math.floor(it.total ? (it.done / it.total) * 100 : 0)}%`;
    case 'delivered': return t('out.delivered');
    case 'declined': return t('out.declined');
    default: return t('out.changed');
  }
}

function cloudLines(items) {
  const up = items.find(i => i.cloud === 'uploading');
  if (up) return `<div class="cloud-up">${progressBlock('cloud')}</div>`;
  if (items.some(i => i.cloud === 'uploaded')) return `<div class="state">${icon('cloud')}${t('out.in_cloud')}</div>`;
  const failed = items.find(i => i.cloud === 'failed');
  if (failed) return `<div class="note err">${icon('warning')}${esc(failed.message || t('out.cloud_failed'))}</div>`;
  return '';
}

function cloudBtn(items) {
  const ids = items.filter(i => !FINAL.includes(i.state) && i.state !== 'downloading' && (i.cloud === 'none' || i.cloud === 'failed')).map(i => i.id);
  if (!S.cloud.connected || !ids.length || items.some(i => i.cloud === 'uploading')) return '';
  const retry = items.some(i => i.cloud === 'failed');
  return `<button class="btn sm subtle" data-act="upload" data-id="${esc(ids.join(','))}">${icon('cloudUp')}${retry ? t('out.upload_retry') : t('out.upload')}</button>`;
}

// Отправленное одному устройству — как раньше; нескольким — одна карточка со строкой на каждого.
function outgoingCard(items) {
  const it = items[0];
  const ids = items.map(i => i.id).join(',');
  const name = `<div class="name" title="${esc(it.item)}">${esc(it.item)}</div>`;
  const badge = it.is_update ? `<span class="badge update">${icon('sync')}${t('in.badge_update')}</span>` : '';
  const done = items.every(i => FINAL.includes(i.state));
  if (items.length === 1) {
    const to = t('out.to', { name: it.peer });
    if (done) {
      const st = it.state === 'delivered' ? `<span class="state ok">${icon('check')}${t('out.delivered')}</span>`
        : it.state === 'declined' ? `<span>${t('out.declined')}</span>` : `<span class="state bad">${icon('warning')}${t('out.changed')}</span>`;
      return `<div class="card compact">${tile(it)}<div class="body">${name}
        <div class="meta">${st}<span>${fmtSize(it.total)}</span><span>${to}</span><span>${ago(it.updated_at)}</span></div></div>${dismissBtn(ids)}</div>`;
    }
    const stateHtml = it.state === 'downloading' ? progressBlock() : `<div class="state wait">${icon('sync')}${outStatus(it)}</div>`;
    return `<div class="card">${tile(it)}<div class="body">${badge}${name}${meta([...sizeMeta(it), to, ago(it.created_at)])}${stateHtml}${cloudLines(items)}
      <div class="actions"><button class="btn sm" data-act="open" data-path="${esc(it.path || '')}">${icon('folderOpen')}${t('btn.show')}</button>${cloudBtn(items)}</div></div></div>`;
  }
  const rows = items.map(i => {
    const cls = i.state === 'delivered' ? 'ok' : FINAL.includes(i.state) ? 'muted' : 'wait';
    const ic = i.state === 'delivered' ? icon('check') : FINAL.includes(i.state) ? '' : icon('sync');
    return `<div class="rcpt ${cls}"><span class="rcpt-name">${esc(i.peer)}</span><span class="rcpt-state">${ic}${outStatus(i, true)}</span></div>`;
  }).join('');
  const delivered = items.filter(i => i.state === 'delivered').length;
  const summary = t('out.delivered_of', { n: delivered, total: items.length });
  if (done) {
    return `<div class="card compact">${tile(it)}<div class="body">${name}
      <div class="meta"><span class="state ${delivered ? 'ok' : ''}">${delivered ? icon('check') : ''}${summary}</span><span>${fmtSize(it.total)}</span><span>${ago(Math.max(...items.map(i => i.updated_at)))}</span></div>
      <div class="rcpts">${rows}</div></div>${dismissBtn(ids)}</div>`;
  }
  return `<div class="card">${tile(it)}<div class="body">${badge}${name}${meta([...sizeMeta(it), tp('out.recipients', items.length), ago(it.created_at)])}
    <div class="rcpts">${rows}</div>${cloudLines(items)}
    <div class="actions"><button class="btn sm" data-act="open" data-path="${esc(it.path || '')}">${icon('folderOpen')}${t('btn.show')}</button>${cloudBtn(items)}</div></div></div>`;
}

// ---------- Сообщения ----------

const URL_RE = /https?:\/\/[^\s<>"'«»]+/gi;
const cleanUrl = u => u.replace(/[.,);!?]+$/, '');
function firstUrl(text) {
  const m = String(text).match(URL_RE);
  return m ? cleanUrl(m[0]) : null;
}

// Текст с кликабельными ссылками (всё остальное экранировано).
function linkify(text) {
  let out = '', last = 0;
  for (const m of text.matchAll(URL_RE)) {
    const url = cleanUrl(m[0]);
    out += esc(text.slice(last, m.index)) + `<a href="#" data-act="open-url" data-url="${esc(url)}">${esc(url)}</a>`;
    last = m.index + url.length;
  }
  return out + esc(text.slice(last));
}

function noteWho(n) {
  if (!n.outgoing) return `<span>${t('in.from', { name: n.peer })}</span>`;
  const status = r => r.delivered ? `${icon('check')}${t('out.delivered')}` : r.needs_update ? t('note.needs_update') : t('note.waiting');
  if (n.to.length === 1) {
    const r = n.to[0];
    return `<span>${t('out.to', { name: r.name })}</span><span class="state ${r.delivered ? 'ok' : 'wait'}">${status(r)}</span>`;
  }
  return `<span>${t('note.to_all')}</span>` + n.to.map(r =>
    `<span class="state ${r.delivered ? 'ok' : 'wait'}">${r.delivered ? icon('check') : ''}${esc(r.name)}${r.delivered ? '' : ' — ' + status(r)}</span>`).join('');
}

function noteCard(n) {
  const url = firstUrl(n.text);
  return `<div class="card note-card ${!n.outgoing && !n.seen ? 'highlight' : ''}"><div class="tile note">${icon('chat')}</div>
    <div class="body"><div class="meta">${noteWho(n)}<span>${ago(n.created_at)}</span></div>
    <div class="note-text selectable">${linkify(n.text)}</div>
    <div class="actions"><button class="btn sm ${n.outgoing ? '' : 'primary'}" data-act="copy-note" data-id="${esc(n.id)}">${icon('copy')}${t('btn.copy')}</button>
    ${url ? `<button class="btn sm" data-act="open-url" data-url="${esc(url)}">${icon('link')}${t('btn.open_link')}</button>` : ''}</div></div>${dismissBtn(n.id)}</div>`;
}

// Окно «Сообщение семье»: текст и кому (всем или одному устройству).
let noteTo = '';
function openComposer(text) {
  if (view !== 'main') closeViews();
  if (noteTo && !S.peers.some(p => p.id === noteTo)) noteTo = '';
  const m = $('#modal');
  const chip = (id, name) => `<button class="to-chip ${noteTo === id ? 'on' : ''}" data-to="${esc(id)}">${id ? `<i style="background:${Papych.colorOf(id)}"></i>` : ''}${esc(name)}</button>`;
  m.innerHTML = `<div class="dialog note-dlg"><div class="content">
      <h3>${t('note.title')}</h3>
      <textarea id="note-text" rows="5" maxlength="20000" placeholder="${esc(t('note.placeholder'))}"></textarea>
      ${S.peers.length ? `<div class="note-to"><span>${t('note.to')}</span>${chip('', t('note.all'))}${S.peers.map(p => chip(p.id, p.name)).join('')}</div>`
        : `<div class="note warn">${icon('warning')}${t('peer.none')}</div>`}
      <small class="hint">${t('note.hint')}</small></div>
    <div class="buttons"><button class="btn primary" data-r="send" ${S.peers.length ? '' : 'disabled'}>${icon('chat')}${t('note.send')}</button>
      <button class="btn" data-r="0">${t('btn.cancel')}</button></div></div>`;
  m.classList.remove('hidden');
  const ta = $('#note-text');
  ta.value = text || '';
  ta.focus();
  ta.setSelectionRange(ta.value.length, ta.value.length);
  ta.addEventListener('input', () => Buddy.watch(ta));
  ta.addEventListener('keydown', e => { if (e.key === 'Enter' && e.ctrlKey) { e.preventDefault(); send(); } });
  m.onclick = e => {
    const to = e.target.closest('[data-to]');
    if (to) {
      noteTo = to.dataset.to;
      m.querySelectorAll('[data-to]').forEach(b => b.classList.toggle('on', b === to));
      return;
    }
    const b = e.target.closest('[data-r]');
    if (b && b.dataset.r === 'send') return send();
    if (b || e.target === m) m.classList.add('hidden');
  };
  async function send() {
    const body = ta.value.trim();
    if (!body) return ta.focus();
    const r = ta.getBoundingClientRect();
    try {
      await invoke('send_note', { text: body, peers: noteTo ? [noteTo] : [] });
    } catch (e) {
      return toast(esc(String(e)), 'err');
    }
    m.classList.add('hidden');
    switchTab('out');
    Buddy.noteSent([r.left + r.width / 2, r.top + r.height / 2], noteTo ? Papych.colorOf(noteTo) : '#60cdff');
  }
}

// Полученные сообщения считаются прочитанными, когда их показали на экране пару секунд.
let seenTimer = 0;
function markSeen() {
  clearTimeout(seenTimer);
  if (tab !== 'in' || view !== 'main' || !Buddy.visible()) return;
  const ids = S.notes.filter(n => !n.outgoing && !n.seen).map(n => n.id);
  if (ids.length) seenTimer = setTimeout(() => invoke('notes_seen', { ids }).catch(() => {}), 2500);
}

function renderKeyed(container, items, emptyHtml) {
  if (!items.length) {
    if (container.dataset.empty !== '1') { container.innerHTML = emptyHtml; container.dataset.empty = '1'; }
    return;
  }
  if (container.dataset.empty === '1') { container.innerHTML = ''; container.dataset.empty = ''; }
  const existing = new Map();
  for (const c of container.children) if (c.dataset.key) existing.set(c.dataset.key, c);
  let prev = null;
  for (const it of items) {
    let el = existing.get(it.key);
    if (el && el.dataset.sig !== it.html) {
      const n = toEl(it.html);
      n.dataset.key = it.key; n.dataset.sig = it.html; n.style.animation = 'none';
      el.replaceWith(n); el = n;
    } else if (!el) {
      el = toEl(it.html);
      el.dataset.key = it.key; el.dataset.sig = it.html;
    }
    existing.delete(it.key);
    const want = prev ? prev.nextElementSibling : container.firstElementChild;
    if (el !== want) container.insertBefore(el, want);
    if (it.update) it.update(el);
    prev = el;
  }
  existing.forEach(el => el.remove());
}

function groups(list) {
  const map = new Map();
  for (const it of list) {
    if (!map.has(it.batch)) map.set(it.batch, []);
    map.get(it.batch).push(it);
  }
  return [...map.values()];
}

function renderLists() {
  const active = i => ['new', 'queued', 'downloading', 'failed'].includes(i.state);
  const inAct = S.incoming.filter(active), inDone = S.incoming.filter(i => !active(i));
  const notesIn = S.notes.filter(n => !n.outgoing);
  // Сверху — новые сообщения и файлы, ждущие ответа; ниже — всё остальное по времени.
  const inItems = [
    ...notesIn.filter(n => !n.seen).map(n => ({ key: n.id, html: noteCard(n) })),
    ...inAct.map(it => ({
      key: it.id, html: incomingCard(it),
      update: it.state === 'downloading' ? el => updateProgress(el, it.done, it.total, it.speed, esc(it.source || '')) : null,
    })),
  ];
  const inEarlier = [
    ...inDone.map(it => ({ at: it.created_at, key: it.id, html: incomingCard(it) })),
    ...notesIn.filter(n => n.seen).map(n => ({ at: n.created_at, key: n.id, html: noteCard(n) })),
  ].sort((a, b) => b.at - a.at);
  if (inEarlier.length) {
    if (inItems.length) inItems.push({ key: 's:in', html: `<div class="section-title">${t('list.earlier')}</div>` });
    inItems.push(...inEarlier);
  }
  renderKeyed($('#list-in'), inItems, `<div class="empty"><div class="art">${icon('arrowDown')}</div>
    <h3>${t('in.empty_title')}</h3><p>${t('in.empty_text')}</p></div>`);

  const all = groups(S.outgoing);
  const outActive = all.filter(g => !g.every(i => FINAL.includes(i.state)));
  const outDone = all.filter(g => g.every(i => FINAL.includes(i.state)));
  const notesOut = S.notes.filter(n => n.outgoing);
  const noteWaits = n => n.to.some(r => !r.delivered);
  const outItems = S.preparing.map(p => ({
    key: 'prep:' + p.item,
    html: `<div class="card">${tile({ kind: 'file' })}<div class="body"><div class="name">${esc(p.item)}</div>
      <div class="state wait">${icon('sync')}${t('out.preparing')}</div>${progressBlock()}</div></div>`,
    update: el => updateProgress(el, p.done, p.total, 0),
  }));
  outActive.forEach(g => outItems.push({
    key: 'b:' + g[0].batch, html: outgoingCard(g),
    update: el => {
      const it = g[0];
      if (g.length === 1 && it.state === 'downloading') updateProgress(el, it.done, it.total, it.speed, t('out.downloading_by', { name: it.peer }));
      const cu = el.querySelector('.cloud-up');
      const up = g.find(i => i.cloud === 'uploading');
      if (cu && up) updateProgress(cu, up.cloud_done, up.total, up.speed, t('out.to_cloud'));
    },
  }));
  notesOut.filter(noteWaits).reverse().forEach(n => outItems.unshift({ key: n.id, html: noteCard(n) }));
  const outEarlier = [
    ...outDone.map(g => ({ at: g[0].created_at, key: 'b:' + g[0].batch, html: outgoingCard(g) })),
    ...notesOut.filter(n => !noteWaits(n)).map(n => ({ at: n.created_at, key: n.id, html: noteCard(n) })),
  ].sort((a, b) => b.at - a.at);
  if (outEarlier.length) {
    if (outItems.length) outItems.push({ key: 's:out', html: `<div class="section-title">${t('list.earlier')}</div>` });
    outItems.push(...outEarlier);
  }
  const who = !S.peers.length ? t('out.who_none') : S.peers.length === 1 ? q(S.peers[0].name) : t('out.who_many');
  renderKeyed($('#list-out'), outItems, `<div class="empty"><div class="art">${icon('arrowUp')}</div>
    <h3>${t('out.empty_title')}</h3><p>${t('out.empty_text', { who: '\u0000' }).replace('\u0000', who)}</p>
    <button class="btn" data-act="open-folder">${icon('folderOpen')}${t('btn.open_folder')}</button></div>`);

  const newCount = S.incoming.filter(i => i.state === 'new').length + notesIn.filter(n => !n.seen).length;
  $('#count-in').textContent = newCount ? String(newCount) : '';
  const busy = outActive.length + S.preparing.length + notesOut.filter(noteWaits).length;
  $('#count-out').textContent = busy ? String(busy) : '';
  markSeen();
}

function providerName(c) {
  return c.provider === 'webdav' ? t('cloud.webdav') : c.provider === 'folder' ? t('cloud.folder') : t('cloud.yandex');
}

function renderCloudChip() {
  const c = S.cloud;
  const chip = $('#cloud-chip');
  // Только значок (внизу ещё «Сообщение»), надпись — во всплывающей подсказке.
  let cls = 'chip icon', ic, text;
  if (!c.connected) { ic = 'cloud'; text = t('chip.no_cloud'); }
  else if (c.error) { cls += ' err'; ic = 'warning'; text = `${t('chip.error')}: ${c.error}`; }
  else if (c.uploading) { cls += ' on'; ic = 'cloudUp'; text = t('chip.uploading'); }
  else { cls += ' on'; ic = 'cloud'; text = providerName(c); }
  chip.className = cls;
  setHtml(chip, icon(ic));
  chip.title = text;
  chip.setAttribute('aria-label', text);
}

// Пауза и готовое обновление — полоса над вкладками.
function renderBanner() {
  let html = '';
  if (S.paused_until) {
    const text = S.paused_until >= 8.64e15 ? t('pause.on')
      : t('pause.until', { time: new Date(S.paused_until).toLocaleTimeString(L._lang, { hour: '2-digit', minute: '2-digit' }) });
    html += `<div class="banner">${icon('pause')}<span>${text}</span><button class="btn sm" data-act="resume">${icon('play')}${t('pause.resume')}</button></div>`;
  }
  if (S.update) {
    html += `<div class="banner accent">${icon('upgrade')}<span>${t('update.ready', { v: S.update })}</span><button class="btn sm primary" data-act="update-install">${t('update.install')}</button></div>`;
  }
  setHtml($('#banner'), html);
  const chip = $('#pause-chip');
  chip.className = `chip icon ${S.paused_until ? 'on warn' : ''}`;
  setHtml(chip, icon(S.paused_until ? 'play' : 'pause'));
  chip.title = S.paused_until ? t('pause.resume') : t('pause.btn');
  chip.setAttribute('aria-label', chip.title);
}

function openPausePop() {
  const pop = $('#pop');
  const opt = (min, label) => `<button data-act="pause" data-min="${min}">${label}</button>`;
  pop.innerHTML = `<div class="pop-title">${t('pause.hint')}</div>${opt(30, t('pause.30m'))}${opt(60, t('pause.1h'))}${opt(180, t('pause.3h'))}${opt('', t('pause.forever'))}`;
  pop.classList.remove('hidden');
}

function moveInk() {
  const a = $(`.tab[data-tab="${tab}"]`);
  const ink = $('#tab-ink');
  ink.style.left = `${a.offsetLeft + 10}px`;
  ink.style.width = `${a.offsetWidth - 20}px`;
}

function switchTab(name) {
  tab = name;
  document.querySelectorAll('.tab').forEach(b => b.classList.toggle('active', b.dataset.tab === name));
  $('#list-in').classList.toggle('hidden', name !== 'in');
  $('#list-out').classList.toggle('hidden', name !== 'out');
  moveInk();
}

function render() {
  if (!S) return;
  if (!S.onboarded && view !== 'onboarding') openOnboarding();
  renderStatus();
  renderBanner();
  renderHistoryShares();
  renderLists();
  renderCloudChip();
  Buddy.update(S);
  if (view === 'settings') renderSettingsDynamic();
  if (view === 'onboarding') renderOnboarding();
}

function renderHistoryShares() {
  setHtml($('#history-shares'), (S.history_shares || []).map(r =>
    `<article class="history-share"><b>${t('toast.joined', { name: r.name })}</b>
      <p>${t('share.added_by', { name: r.added_by })}</p>
      <p>${t('share.question', { name: r.name, n: r.files })}</p>
      <small>${t('share.own_only')}</small>
      <div class="actions"><button class="btn sm primary" data-act="share-history" data-id="${esc(r.peer_id)}">${t('share.allow')}</button>
        <button class="btn sm" data-act="keep-private" data-id="${esc(r.peer_id)}">${t('share.deny')}</button></div>
    </article>`).join(''));
}

// ---------- Настройки ----------

const sw = (id, on) => `<label class="switch"><input type="checkbox" id="${id}" ${on ? 'checked' : ''}><span></span></label>`;
const opt = (v, text, cur) => `<option value="${esc(v)}" ${String(cur) === String(v) ? 'selected' : ''}>${text}</option>`;
const row = (title, hint, control) => `<div class="row"><div class="label"><b>${title}</b>${hint ? `<small>${hint}</small>` : ''}</div>${control}</div>`;

function langSelect(id) {
  const langs = L._langs || [];
  return `<select id="${id}" class="lang">${opt('', t('lang.system'), S.settings.language)}${langs.map(([c, n]) => opt(c, esc(n), S.settings.language)).join('')}</select>`;
}

function openSettings(focus) {
  view = 'settings';
  const el = $('#settings');
  el.classList.remove('hidden');
  $('#about').classList.add('hidden');
  $('#main').classList.add('hidden');
  const st = S.settings;
  const speeds = [0, 512, 1024, 5120, 10240, 25600];
  const limits = [10, 100, 1024, 5120, 20480, 0];
  el.innerHTML = `
    <div class="view-head"><button class="tb-btn" data-act="back" title="${t('btn.back')}">${icon('back')}</button><h2>${t('set.title')}</h2></div>
    <div class="view-body">
      <div class="group-title">${t('set.this_pc')}</div>
      <div class="row col"><div class="field"><label>${t('set.name')}</label>
        <input type="text" id="set-name" maxlength="40" value="${esc(st.device_name)}"></div></div>
      <div class="row"><div class="label"><b>${t('set.folder')}</b><small class="selectable" title="${esc(S.folder)}">${esc(S.folder)}</small></div>
        <button class="btn sm" data-act="pick-folder">${t('btn.change')}</button></div>

      <div class="group-title" id="family-title">${t('set.family')}</div>
      <div id="set-peers" class="stack"></div>
      <div class="row col" id="set-invite"></div>

      <div class="group-title">${t('set.receiving')}</div>
      ${row(t('recv.auto'), t('recv.auto_hint'), `<select id="set-auto" style="width:150px">${opt('Off', t('recv.off'), st.auto_accept)}${opt('Photos', t('recv.photos'), st.auto_accept)}${opt('All', t('recv.all'), st.auto_accept)}</select>`)}
      <div id="auto-extra" class="stack ${st.auto_accept === 'Off' ? 'hidden' : ''}">
        ${row(t('recv.limit'), t('recv.limit_hint'), `<select id="set-auto-mb" style="width:150px">${limits.map(v => opt(v, v ? fmtSize(v * 1048576) : t('recv.unlimited'), st.auto_accept_mb)).join('')}</select>`)}
        ${row(t('recv.exe'), t('recv.exe_hint'), sw('set-auto-exe', st.auto_accept_exe))}
      </div>
      ${row(t('set.speed'), t('set.speed_hint'), `<select id="set-speed" style="width:150px">${speeds.map(v => opt(v, v ? t('speed', { v: fmtSize(v * 1024) }) : t('speed.none'), st.speed_limit_kbps)).join('')}</select>`)}

      <div class="group-title" id="cloud-title">${t('set.cloud')}</div>
      <div id="set-cloud" class="stack"></div>

      <div class="group-title">${t('set.explorer')}</div>
      ${row(t('set.pin'), t('set.pin_hint'), `<button class="btn sm" data-act="pin">${t('btn.pin')}</button>`)}
      ${row(t('set.shortcut'), '', `<button class="btn sm" data-act="shortcut">${t('btn.create')}</button>`)}
      ${row(t('set.sendto'), t('set.sendto_hint'), `<button class="btn sm" data-act="sendto">${t('btn.add')}</button>`)}

      <div class="group-title">${t('set.island_group')}</div>
      ${row(t('set.island'), t('set.island_hint'), sw('set-island', st.island))}
      <div id="island-extra" class="stack ${st.island ? '' : 'hidden'}">
        ${row(t('set.notify_via'), t('set.notify_via_hint'), `<select id="set-notify-via" style="width:150px">${opt('Island', t('notify.island'), st.notify_via)}${opt('Windows', t('notify.windows'), st.notify_via)}${opt('Both', t('notify.both'), st.notify_via)}</select>`)}
      </div>
      ${row(t('set.notify'), t('set.notify_hint'), sw('set-notify', st.notify_delivered))}

      <div class="group-title">${t('set.papych')}</div>
      ${row(t('set.tour'), t('set.tour_hint'), `<button class="btn sm" data-act="tour">${t('btn.show_tour')}</button>`)}
      ${row(t('set.tips'), t('set.tips_hint'), sw('set-tips', st.tips))}

      <div class="group-title">${t('set.updates')}</div>
      ${row(t('update.auto'), t('update.auto_hint'), sw('set-auto-update', st.auto_update))}
      <div id="set-update"></div>

      <div class="group-title">${t('set.other')}</div>
      ${row(t('set.language'), '', langSelect('set-lang'))}
      ${row(t('set.autostart'), t('set.autostart_hint'), sw('set-autostart', st.autostart))}
      ${row(t('set.dock'), t('set.dock_hint'), sw('set-dock', st.dock_panel))}
      ${row(t('set.logs'), t('set.logs_hint'), `<button class="btn sm" data-act="logs">${t('btn.open')}</button>`)}
      <button class="row link-row" data-act="about">${icon('shield', 'lg')}<div class="label"><b>${t('set.about')}</b><small>${t('set.about_hint')}</small></div>${icon('back', 'flip')}</button>
      <div class="row"><div class="label"><b>${t('set.version', { v: S.version })}</b><small class="selectable">${t('set.device_code', { code: S.me.slice(0, 16) })}</small></div></div>
      <button class="btn wide danger" data-act="quit" style="margin-top:12px">${t('btn.quit')}</button>
    </div>`;
  invite = null; joinOpen = false;
  renderSettingsDynamic();

  $('#set-name').addEventListener('change', e => saveSettings({ device_name: e.target.value.trim() || S.settings.device_name }));
  $('#set-auto').addEventListener('change', e => {
    $('#auto-extra').classList.toggle('hidden', e.target.value === 'Off');
    saveSettings({ auto_accept: e.target.value });
  });
  $('#set-auto-mb').addEventListener('change', e => saveSettings({ auto_accept_mb: Number(e.target.value) }));
  $('#set-auto-exe').addEventListener('change', e => saveSettings({ auto_accept_exe: e.target.checked }));
  $('#set-speed').addEventListener('change', e => saveSettings({ speed_limit_kbps: Number(e.target.value) }));
  $('#set-lang').addEventListener('change', e => changeLanguage(e.target.value));
  $('#set-auto-update').addEventListener('change', e => saveSettings({ auto_update: e.target.checked }));
  $('#set-autostart').addEventListener('change', e => saveSettings({ autostart: e.target.checked }));
  $('#set-dock').addEventListener('change', e => saveSettings({ dock_panel: e.target.checked }));
  $('#set-notify').addEventListener('change', e => saveSettings({ notify_delivered: e.target.checked }));
  $('#set-island').addEventListener('change', e => {
    $('#island-extra').classList.toggle('hidden', !e.target.checked);
    saveSettings({ island: e.target.checked });
  });
  $('#set-notify-via').addEventListener('change', e => saveSettings({ notify_via: e.target.value }));
  $('#set-tips').addEventListener('change', e => saveSettings({ tips: e.target.checked }));
  const scrollTo = { cloud: '#cloud-title', invite: '#set-invite', family: '#family-title' }[focus];
  if (scrollTo) setTimeout(() => $(scrollTo).scrollIntoView({ behavior: 'smooth' }), 50);
}

function openAbout() {
  view = 'about';
  const el = $('#about');
  $('#settings').classList.add('hidden');
  el.classList.remove('hidden');
  el.innerHTML = `
    <div class="view-head"><button class="tb-btn" data-act="settings" title="${t('btn.back')}">${icon('back')}</button><h2>${t('about.title')}</h2></div>
    <div class="view-body">
      <div class="about-hero"><img src="logo.png" alt=""><div><b>${t('app.name')}</b><small>${t('set.version', { v: S.version })}</small></div></div>
      <p class="about-text">${t('app.tagline')}</p>
      <div class="group-title">${t('about.privacy_title')}</div>
      <div class="row col legal">${paragraphs(t('legal.privacy'))}</div>
      <div class="group-title">${t('about.terms_title')}</div>
      <div class="row col legal">${paragraphs(t('legal.terms'))}</div>
      <div class="group-title">${t('about.license_title')}</div>
      <div class="row col legal"><p>${t('about.license')}</p></div>
    </div>`;
}

function closeViews() {
  view = 'main';
  $('#btn-settings').classList.remove('hidden');
  ['#settings', '#about', '#onboarding'].forEach(s => $(s).classList.add('hidden'));
  $('#main').classList.remove('hidden');
  render();
  moveInk();
}

async function saveSettings(patch) {
  const settings = { ...S.settings, ...patch };
  await run(invoke('save_settings', { settings }));
  S.settings = settings;
}

async function changeLanguage(language) {
  await saveSettings({ language });
  L = await invoke('get_strings');
  applyStatic();
  const v = view;
  if (v === 'settings') openSettings();
  else if (v === 'onboarding') renderOnboarding(true);
  render();
  moveInk();
}

function renderDevices() {
  const peers = $('#set-peers');
  setHtml(peers, S.peers.length
    ? S.peers.map(p => `<div class="row"><i class="peer-dot ${p.online ? 'on' : ''}"></i><div class="label"><b>${esc(p.name)}</b>
        <small>${peerNote(p)}</small>${p.outdated ? `<small class="warn-text">${t('peer.outdated', { v: p.version })}</small>` : ''}</div>
        <button class="tb-btn" title="${t('set.remove_device')}" data-act="remove-peer" data-id="${esc(p.id)}" data-name="${esc(p.name)}">${icon('trash')}</button></div>`).join('')
    : `<div class="row"><div class="label"><small>${t('set.no_devices')}</small></div></div>`);

  let inv;
  if (invite) {
    inv = `<div class="field"><label>${t('inv.code')}</label><div class="code-box">${esc(invite)}</div></div>
      <div class="actions"><button class="btn sm primary" data-act="copy-invite">${icon('copy')}${t('btn.copy')}</button>
      <button class="btn sm subtle" data-act="close-invite">${t('btn.done')}</button></div>
      <ol class="steps"><li>${t('inv.step1')}</li><li>${t('inv.step2')}</li><li>${t('inv.step3')}</li></ol>`;
  } else if (joinOpen) {
    inv = `<div class="field"><label>${t('join.label')}</label><textarea id="join-code" rows="3" placeholder="ABCD-EFGH-…"></textarea></div><small class="hint">${t('join.hint')}</small>
      <div class="actions"><button class="btn sm primary" data-act="join">${t('btn.connect')}</button><button class="btn sm subtle" data-act="close-invite">${t('btn.cancel')}</button></div>`;
  } else {
    inv = `<div class="actions" style="margin:0"><button class="btn primary" data-act="invite">${icon('add')}${t('set.invite_btn')}</button>
      <button class="btn" data-act="open-join">${icon('link')}${t('set.have_code')}</button></div>
      <small class="hint">${t('set.family_hint')}</small>`;
  }
  const invEl = $('#set-invite');
  if (invEl._html !== inv) {
    const keep = $('#join-code')?.value;
    setHtml(invEl, inv);
    if (keep && $('#join-code')) $('#join-code').value = keep;
  }
}

const WEBDAV_PRESETS = [
  ['nextcloud', 'Nextcloud / ownCloud', 'https://SERVER/remote.php/dav/files/USER/'],
  ['koofr', 'Koofr', 'https://app.koofr.net/dav/Koofr'],
  ['pcloud', 'pCloud', 'https://webdav.pcloud.com'],
  ['yandex', 'Yandex Disk (WebDAV)', 'https://webdav.yandex.ru'],
  ['other', null, ''],
];

function cloudHtml() {
  const c = S.cloud;
  if (c.auth_code) {
    return `<div class="row col"><p class="text">${t('cloud.ya_enter')}</p>
      <div class="code-box big-code">${esc(c.auth_code)}</div>
      <div class="actions"><button class="btn primary" data-act="open-url" data-url="${esc(c.auth_url)}">${t('cloud.ya_open')}</button>
      <button class="btn subtle" data-act="cloud-off">${t('btn.cancel')}</button></div>
      <div class="state wait">${icon('sync')}${t('cloud.ya_wait')}</div></div>`;
  }
  const err = c.error ? `<div class="note err">${icon('warning')}${esc(c.error)}</div>` : '';
  if (c.connected) {
    const m = S.settings.cloud_mode;
    const radio = (v, title, sub) => `<label class="radio"><input type="radio" name="cloud-mode" value="${v}" ${m === v ? 'checked' : ''}><div>${title}<small>${sub}</small></div></label>`;
    return `<div class="row">${icon('cloud', 'lg')}<div class="label"><b>${esc(providerName(c))}</b><small class="selectable" title="${esc(c.login)}">${esc(c.login)}</small></div>
        <button class="btn sm" data-act="cloud-off">${t('btn.disconnect')}</button></div>
      ${err}
      <div class="row col" id="cloud-mode">
        ${radio('WhenNeeded', t('cloud.mode_when'), t('cloud.mode_when_hint'))}
        ${radio('Always', t('cloud.mode_always'), t('cloud.mode_always_hint'))}
        ${radio('Never', t('cloud.mode_never'), t('cloud.mode_never_hint'))}
      </div>
      <div class="row"><div class="label"><small>${t('cloud.note')}${c.local ? ' ' + t('cloud.local_note') : ''}</small></div></div>`;
  }
  const tabBtn = (id, title, sub) => `<button class="seg ${cloudTab === id ? 'on' : ''}" data-act="cloud-tab" data-id="${id}"><b>${title}</b><small>${sub}</small></button>`;
  let body;
  if (cloudTab === 'yandex') {
    body = `<details><summary>${t('cloud.ya_how')}</summary>
        <ol class="steps"><li>${t('cloud.ya_step1')}</li><li>${t('cloud.ya_step2')}</li><li>${t('cloud.ya_step3')}</li><li>${t('cloud.ya_step4')}</li></ol></details>
      <div class="field"><label>ClientID</label><input type="text" id="cloud-id" spellcheck="false" value="${esc(c.client_id)}"></div>
      <div class="field"><label>Client secret</label><input type="password" id="cloud-secret" spellcheck="false" value="${esc(c.client_secret)}"></div>
      ${err}<div class="actions"><button class="btn primary" data-act="cloud-connect">${t('cloud.ya_connect')}</button></div>
      <small class="hint">${t('cloud.one_enough')}</small>`;
  } else if (cloudTab === 'webdav') {
    body = `<div class="field"><label>${t('cloud.dav_service')}</label><select id="dav-preset">${WEBDAV_PRESETS.map(([id, name, url]) => `<option value="${id}" ${(c.url ? url === c.url : id === 'other') ? 'selected' : ''}>${esc(name || t('cloud.dav_other'))}</option>`).join('')}</select></div>
      <div class="field"><label>${t('cloud.dav_url')}</label><input type="text" id="dav-url" spellcheck="false" value="${esc(c.url)}" placeholder="https://"></div>
      <div class="field"><label>${t('cloud.dav_user')}</label><input type="text" id="dav-user" spellcheck="false" value="${esc(c.user)}"></div>
      <div class="field"><label>${t('cloud.dav_pass')}</label><input type="password" id="dav-pass" spellcheck="false"></div>
      <small class="hint">${t('cloud.dav_hint')}</small>
      ${err}<div class="actions"><button class="btn primary" data-act="dav-connect">${t('btn.connect')}</button></div>
      <small class="hint">${t('cloud.one_enough')}</small>`;
  } else {
    const found = cloudFolders === null ? `<div class="state wait">${icon('sync')}${t('cloud.folder_search')}</div>`
      : cloudFolders.length ? cloudFolders.map(([name, path]) => `<div class="row inner"><div class="label"><b>${esc(name)}</b><small class="selectable" title="${esc(path)}">${esc(path)}</small></div>
          <button class="btn sm primary" data-act="cloud-folder" data-path="${esc(path)}">${t('cloud.folder_use')}</button></div>`).join('')
      : `<small class="hint">${t('cloud.folder_none')}</small>`;
    body = `<p class="text">${t('cloud.folder_text')}</p>${found}
      <div class="actions"><button class="btn" data-act="cloud-folder-pick">${icon('folderOpen')}${t('cloud.folder_pick')}</button></div>
      <small class="hint">${t('cloud.folder_note')}</small>${err}`;
  }
  return `<div class="row col"><p class="text">${t('cloud.intro')}</p>
    <div class="segs">${tabBtn('yandex', t('cloud.yandex'), t('cloud.yandex_sub'))}${tabBtn('webdav', t('cloud.webdav'), t('cloud.webdav_sub'))}${tabBtn('folder', t('cloud.folder'), t('cloud.folder_sub'))}</div>
    ${body}</div>`;
}

function renderSettingsDynamic() {
  if (!$('#set-peers')) return;
  renderDevices();
  setHtml($('#set-update'), S.update
    ? row(t('update.ready', { v: S.update }), t('update.current', { v: S.version }), `<button class="btn sm primary" data-act="update-install">${t('update.install')}</button>`)
    : row(t('update.current', { v: S.version }), '', ''));

  const cloud = cloudHtml();
  const cEl = $('#set-cloud');
  if (cEl._html !== cloud) {
    const keep = {};
    cEl.querySelectorAll('input[id], select[id]').forEach(i => (keep[i.id] = i.value));
    const open = cEl.querySelector('details')?.open;
    setHtml(cEl, cloud);
    for (const [id, v] of Object.entries(keep)) if (v && $('#' + id)) $('#' + id).value = v;
    if (open && cEl.querySelector('details')) cEl.querySelector('details').open = true;
    cEl.querySelectorAll('input[name="cloud-mode"]').forEach(r => r.addEventListener('change', e => saveSettings({ cloud_mode: e.target.value })));
    const preset = $('#dav-preset');
    if (preset) preset.onchange = () => {
      const p = WEBDAV_PRESETS.find(x => x[0] === preset.value);
      $('#dav-url').value = p ? p[2] : '';
      $('#dav-url').focus();
    };
  }
}

// ---------- Знакомство (первый запуск) ----------

function openOnboarding() {
  view = 'onboarding';
  $('#btn-settings').classList.add('hidden');
  onb.name = onb.name || S.settings.device_name;
  onb.folder = onb.folder || S.folder;
  $('#onboarding').classList.remove('hidden');
  ['#main', '#settings', '#about'].forEach(s => $(s).classList.add('hidden'));
  renderOnboarding(true);
}

function renderOnboarding(force) {
  const el = $('#onboarding');
  const dots = n => `<div class="dots">${[0, 1, 2].map(i => `<i class="${i === n ? 'on' : ''}"></i>`).join('')}</div>`;
  let html;
  if (onb.step === 0) {
    html = `<div class="onb"><div class="lang-pick">${icon('globe')}${langSelect('onb-lang')}</div>
      <div class="hero"><img src="logo.png" alt=""><h1>${t('app.name')}</h1><p>${t('onb.welcome_text')}</p></div>
      <div class="bottom">${dots(0)}<button class="btn primary wide" data-act="onb-next">${t('onb.start')}</button>
      <small class="legal-line">${t('onb.legal', { link: '\u0000' }).replace('\u0000', `<a href="#" data-act="legal">${t('onb.legal_link')}</a>`)}</small></div></div>`;
  } else if (onb.step === 1) {
    html = `<div class="onb"><h1>${t('onb.this_pc')}</h1>
      <div class="field"><label>${t('onb.name')}</label><input type="text" id="onb-name" maxlength="40" value="${esc(onb.name)}"></div>
      <div class="field"><label>${t('onb.folder')}</label>
        <div class="row" style="min-height:0;padding:8px 10px"><div class="label"><small class="selectable" title="${esc(onb.folder)}">${esc(onb.folder)}</small></div>
        <button class="btn sm" data-act="onb-folder">${t('btn.change')}</button></div></div>
      <label class="check"><input type="checkbox" id="onb-shortcut" ${onb.shortcut ? 'checked' : ''}>${t('onb.shortcut')}</label>
      <label class="check"><input type="checkbox" id="onb-pin" ${onb.pin ? 'checked' : ''}>${t('onb.pin')}</label>
      <div class="bottom">${dots(1)}<button class="btn primary wide" data-act="onb-setup" ${onb.busy ? 'disabled' : ''}>${t('btn.next')}</button></div></div>`;
  } else if (onb.mode === 'new') {
    const joined = S.peers.find(p => p.online);
    html = `<div class="onb"><h1>${t('onb.invite_title')}</h1>
      <p>${t('onb.invite_text')}</p>
      <div class="code-box">${invite ? esc(invite) : t('onb.creating')}</div>
      <button class="btn" data-act="copy-invite" ${invite ? '' : 'disabled'}>${icon('copy')}${t('onb.copy_code')}</button>
      <p style="font-size:12px">${t('onb.code_note')}</p>
      ${joined ? `<div class="note">${icon('check')}${t('onb.connected', { name: joined.name })}</div>` : ''}
      <div class="bottom">${dots(2)}<button class="btn primary wide" data-act="onb-done">${t('btn.done')}</button></div></div>`;
  } else if (onb.mode === 'join') {
    html = `<div class="onb"><h1>${t('onb.join_title')}</h1>
      ${onb.joined
        ? `<div class="note">${icon('check')}${t('onb.joined', { name: onb.joined })}</div>`
        : `<p>${t('onb.paste')}</p>
           <textarea id="onb-code" rows="4" placeholder="ABCD-EFGH-…">${esc(onb.code)}</textarea><small class="hint">${t('join.hint')}</small>
           <button class="btn primary" data-act="onb-join" ${onb.busy ? 'disabled' : ''}>${onb.busy ? t('btn.connecting') : t('btn.connect')}</button>`}
      <div class="bottom">${dots(2)}${onb.joined ? `<button class="btn primary wide" data-act="onb-done">${t('btn.done')}</button>`
        : `<button class="btn subtle wide" data-act="onb-back">${t('btn.back')}</button>`}</div></div>`;
  } else {
    html = `<div class="onb"><h1>${t('onb.first')}</h1>
      <button class="choice" data-act="onb-new"><div class="tile">${icon('add')}</div><div><b>${t('onb.new')}</b><small>${t('onb.new_sub')}</small></div></button>
      <button class="choice" data-act="onb-join-mode"><div class="tile">${icon('link')}</div><div><b>${t('onb.join')}</b><small>${t('onb.join_sub')}</small></div></button>
      <div class="bottom">${dots(2)}</div></div>`;
  }
  if (force || el._html !== html) {
    const code = $('#onb-code')?.value;
    setHtml(el, html);
    if (code !== undefined && $('#onb-code')) $('#onb-code').value = code;
    const lang = $('#onb-lang');
    if (lang) lang.onchange = () => changeLanguage(lang.value);
  }
}

// ---------- Действия ----------

async function act(btn) {
  const a = btn.dataset.act, id = btn.dataset.id;
  switch (a) {
    case 'note-new': return openComposer('');
    case 'tour': closeViews(); return run(invoke('tour_start'));
    case 'copy-note': {
      const n = S.notes.find(x => x.id === id);
      if (!n) return;
      await run(invoke('copy_text', { text: n.text }));
      Buddy.copied();
      toast(t('note.copied'));
      if (!n.outgoing && !n.seen) invoke('notes_seen', { ids: [n.id] }).catch(() => {});
      return;
    }
    case 'open-url': return run(invoke('open_url', { url: btn.dataset.url }));
    case 'accept': return Buddy.press(btn, () => run(invoke('accept', { id })).catch(() => {}));
    case 'share-history':
    case 'keep-private': {
      btn.disabled = true;
      try { await run(invoke('share_history', { peer: id, allow: btn.dataset.act === 'share-history' })); }
      finally { btn.disabled = false; }
      return;
    }
    case 'decline': Buddy.declined(); return run(invoke('decline', { id }));
    case 'dismiss': return Promise.all(id.split(',').map(i => invoke('dismiss', { id: i })));
    case 'upload':
      await Promise.all(id.split(',').map(i => invoke('upload_now', { id: i })));
      return toast(t('toast.uploading'));
    case 'open': return run(invoke('open_path', { path: btn.dataset.path }));
    case 'open-folder': return run(invoke('open_folder'));
    case 'open-url': return run(invoke('open_url', { url: btn.dataset.url }));
    case 'back': return closeViews();
    case 'settings': return openSettings();
    case 'settings-family': return openSettings('family');
    case 'about': return openAbout();
    case 'legal': return legalDialog();
    case 'invite':
      if (view !== 'settings') openSettings('invite');
      invite = await run(invoke('create_invite'));
      joinOpen = false;
      return renderSettingsDynamic();
    case 'open-join': joinOpen = true; invite = null; renderSettingsDynamic(); return $('#join-code')?.focus();
    case 'close-invite': invite = null; joinOpen = false; return renderSettingsDynamic();
    case 'copy-invite':
      await navigator.clipboard.writeText(invite);
      return toast(t('toast.code_copied'));
    case 'join': {
      const code = $('#join-code').value.trim();
      if (!code) return;
      btn.disabled = true;
      try {
        const name = await run(invoke('join', { code }));
        joinOpen = false;
        renderSettingsDynamic();
        toast(t('toast.joined_to', { name }));
      } finally { btn.disabled = false; }
      return;
    }
    case 'remove-peer':
      if (await confirmDialog(t('dlg.remove_title'), t('dlg.remove_text', { name: btn.dataset.name }), t('btn.disconnect'), true))
        await run(invoke('remove_device', { id }));
      return;
    case 'pick-folder': {
      const folder = await invoke('pick_folder', { title: null });
      if (folder) { await saveSettings({ folder }); openSettings(); }
      return;
    }
    case 'cloud-tab':
      cloudTab = id;
      if (id === 'folder' && cloudFolders === null) invoke('cloud_folders').then(f => { cloudFolders = f; renderSettingsDynamic(); });
      return renderSettingsDynamic();
    case 'cloud-connect': {
      const clientId = $('#cloud-id').value.trim(), clientSecret = $('#cloud-secret').value.trim();
      if (!clientId || !clientSecret) return toast(t('cloud.err.need_client'), 'err');
      btn.disabled = true;
      try { await run(invoke('cloud_connect', { clientId, clientSecret })); } finally { btn.disabled = false; }
      return;
    }
    case 'dav-connect': {
      const url = $('#dav-url').value.trim(), user = $('#dav-user').value.trim(), password = $('#dav-pass').value;
      if (!url || url.includes('SERVER')) return toast(t('cloud.err.webdav_url'), 'err');
      btn.disabled = true; btn.textContent = t('btn.connecting');
      try { await run(invoke('cloud_connect_webdav', { url, user, password }), t('toast.cloud_connected')); }
      finally { btn.disabled = false; btn.textContent = t('btn.connect'); }
      return;
    }
    case 'cloud-folder':
      return run(invoke('cloud_use_folder', { path: btn.dataset.path }), t('toast.cloud_connected'));
    case 'cloud-folder-pick': {
      const path = await invoke('pick_folder', { title: t('cloud.folder_pick') });
      if (path) await run(invoke('cloud_use_folder', { path }), t('toast.cloud_connected'));
      return;
    }
    case 'cloud-off':
      if (S.cloud.auth_code || await confirmDialog(t('dlg.cloud_off_title'), t('dlg.cloud_off_text'), t('btn.disconnect')))
        await run(invoke('cloud_disconnect'));
      return;
    case 'shortcut': return run(invoke('create_shortcut'), t('toast.shortcut'));
    case 'pin': return run(invoke('pin_folder'), t('toast.pinned'));
    case 'sendto': return run(invoke('add_send_to'), t('toast.sendto'));
    case 'logs': return run(invoke('open_logs'));
    case 'quit': return askQuit(false);
    case 'pause-menu': return S.paused_until ? run(invoke('resume')) : openPausePop();
    case 'pause':
      $('#pop').classList.add('hidden');
      return run(invoke('pause', { minutes: btn.dataset.min ? Number(btn.dataset.min) : null }));
    case 'resume': return run(invoke('resume'));
    case 'update-install':
      btn.disabled = true;
      toast(t('update.installing'));
      return run(invoke('update_install')).catch(() => { btn.disabled = false; });
    // Знакомство
    case 'onb-next': onb.step = 1; return renderOnboarding();
    case 'onb-folder': {
      const f = await invoke('pick_folder', { title: null });
      if (f) { onb.folder = f; renderOnboarding(); }
      return;
    }
    case 'onb-setup': {
      onb.name = $('#onb-name').value.trim() || onb.name;
      onb.shortcut = $('#onb-shortcut').checked;
      onb.pin = $('#onb-pin').checked;
      onb.busy = true; renderOnboarding();
      try {
        await run(invoke('setup_device', { name: onb.name, folder: onb.folder, shortcut: onb.shortcut, pin: onb.pin }));
        onb.step = 2;
      } finally { onb.busy = false; renderOnboarding(); }
      return;
    }
    case 'onb-new':
      onb.mode = 'new'; renderOnboarding();
      invite = await run(invoke('create_invite'));
      return renderOnboarding();
    case 'onb-join-mode': onb.mode = 'join'; return renderOnboarding();
    case 'onb-back': onb.mode = null; return renderOnboarding();
    case 'onb-join': {
      onb.code = $('#onb-code').value.trim();
      if (!onb.code) return;
      onb.busy = true; renderOnboarding();
      try { onb.joined = await run(invoke('join', { code: onb.code })); } catch { /* показано */ }
      onb.busy = false; renderOnboarding();
      return;
    }
    case 'onb-done':
      await run(invoke('complete_onboarding'));
      invite = null;
      return closeViews();
  }
}

async function askQuit(fromTray) {
  const ok = fromTray ? false : await invoke('quit', { force: false });
  if (!ok && await confirmDialog(t('dlg.quit_title'), t('dlg.quit_text'), t('dlg.quit_ok'), true))
    await invoke('quit', { force: true });
}

// ---------- Запуск ----------

async function init() {
  L = await invoke('get_strings');
  applyStatic();
  $('#btn-settings').innerHTML = icon('settings');
  $('#btn-hide').innerHTML = icon('minimize');
  document.querySelectorAll('[data-icon]').forEach(i => (i.innerHTML = icon(i.dataset.icon)));
  $('#btn-settings').onclick = () => (view === 'settings' || view === 'about' ? closeViews() : openSettings());
  $('#btn-hide').onclick = () => invoke('hide_panel');
  $('#btn-open-folder').onclick = () => run(invoke('open_folder'));
  $('#btn-note').title = t('note.title');
  $('#cloud-chip').onclick = () => openSettings('cloud');
  $('#pause-chip').dataset.act = 'pause-menu';
  document.querySelectorAll('.tab').forEach(b => (b.onclick = () => switchTab(b.dataset.tab)));
  document.addEventListener('click', e => {
    if (!e.target.closest('#pop, #pause-chip')) $('#pop').classList.add('hidden');
    const b = e.target.closest('[data-act]');
    if (b) { e.preventDefault(); act(b).catch(() => {}); }
  });
  document.addEventListener('keydown', e => {
    if (e.key === 'Escape') {
      if (!$('#modal').classList.contains('hidden')) return $('#modal').classList.add('hidden');
      if (!$('#pop').classList.contains('hidden')) return $('#pop').classList.add('hidden');
      if (view === 'about') openSettings();
      else if (view === 'settings') closeViews();
      else invoke('hide_panel');
    }
  });
  document.addEventListener('paste', e => {
    if (view !== 'main' || e.target.closest('input, textarea') || !$('#modal').classList.contains('hidden')) return;
    const text = e.clipboardData && e.clipboardData.getData('text/plain').trim();
    if (!text) return;
    e.preventDefault();
    openComposer(text);
    Buddy.speak(t('note.caught'));
  });
  document.addEventListener('contextmenu', e => { if (!e.target.closest('input, textarea, .selectable, .code-box')) e.preventDefault(); });

  if (await invoke('backdrop').catch(() => false)) document.documentElement.classList.add('mica');
  Buddy.init();

  S = await invoke('get_state');
  render();
  requestAnimationFrame(moveInk);
  await T.event.listen('state', e => { S = e.payload; render(); });
  await T.event.listen('show-tab', e => { if (view !== 'onboarding') { closeViews(); switchTab(e.payload); } });
  await T.event.listen('ask-quit', () => askQuit(true));
  setInterval(() => { if (view === 'main') { renderLists(); renderStatus(); renderBanner(); } }, 30000); // обновить «5 мин назад»
}

init();
