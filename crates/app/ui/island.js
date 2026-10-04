'use strict';
// Шторка сверху экрана. Окно открывает программа (island.rs) и передаёт положение курсора,
// а когда спрятаться, решает эта страница.

const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const $ = sel => document.querySelector(sel);

let L = {};             // строки на текущем языке
let S = null;           // состояние программы
let mode = null;        // peek | drag | offer | note | received | delivered | joined; null — спрятана
let demo = false;       // показ на экскурсии с Папычем: прячется по команде, а не от курсора
let demoTimers = [];
let offers = [];        // новые файлы, ждущие ответа
let until = 0;          // когда спрятать сообщение
let leftAt = 0;         // когда курсор ушёл со шторки
let cursor = { x: -1, y: -1, down: false };
let pass = null;        // прозрачная часть окна пропускает мышь
let dropped = false, releasedAt = 0;
let members = [];       // мини-Папычи семьи, пока тащат файлы
let target = null;      // на кого бросят: null — всем
let dropBounds = '';

const isl = $('#isl');
const bot = new Papych($('#bot'), { variant: 'robot', crop: true });
Papych.active(false);

function esc(s) {
  return String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}
function t(key, p) {
  const s = typeof L[key] === 'string' ? L[key] : key;
  return p ? s.replace(/\{(\w+)\}/g, (m, k) => (k in p ? esc(p[k]) : m)) : s;
}

const hueOf = Papych.hueOf, colorOf = Papych.colorOf;
const baseName = p => String(p).split(/[\\/]/).pop();

function head(title, sub) {
  $('#head').innerHTML = title;
  $('#sub').innerHTML = sub || '';
}

// Кнопки справа: [надпись или значок, действие, класс, подсказка].
function acts(list) {
  const el = $('#acts');
  el.innerHTML = '';
  for (const [label, fn, cls, title] of list) {
    const b = document.createElement('button');
    b.className = cls || '';
    b.innerHTML = label;
    if (title) b.title = title;
    b.onclick = e => { e.stopPropagation(); fn(); };
    el.append(b);
  }
}

// ---------- режимы ----------

function open(reason, data) {
  // Экскурсия показывает шторку: выглянувшую или раскрытую, как при перетаскивании файлов.
  if (reason === 'demo') {
    if (data.kind === 'close') return close();
    demoTimers.forEach(clearTimeout);
    demoTimers = [];
    if (mode === 'drag') mode = null;   // иначе «выглянуть» после раскрытой шторки не даст защита от перебивания
    open(data.kind);
    demo = true;
    if (data.kind === 'drag') demoDrag();
    return;
  }
  demo = false;
  // Пока бросают файлы, сообщения не перебивают: новые файлы просто встанут в очередь.
  if (mode === 'drag' && reason !== 'drag') {
    if (reason === 'offer') addOffer(data);
    return;
  }
  Papych.active(true);
  pass = false;
  isl.classList.add('in');
  mode = reason;
  isl.dataset.mode = reason;
  isl.classList.toggle('drag', reason === 'drag');
  leftAt = 0; dropped = false; releasedAt = 0; until = 0;
  $('#family').innerHTML = '';
  members.forEach(m => m.p.destroy());
  members = [];
  const now = Date.now();
  switch (reason) {
    case 'peek':
      renderPeek();
      bot.wave();
      break;
    case 'drag':
      renderDrag();
      break;
    case 'offer':
      addOffer(data);
      until = now + 12000;
      renderOffer();
      bot.receive({ name: data.title });
      break;
    case 'received':
      until = now + 6000;
      head(t('toast.received', { name: data.title }), data.conflicts ? t('toast.conflict') : t('toast.saved'));
      acts([[t('btn.show'), () => invoke('open_path', { path: data.path })]]);
      bot.done();
      break;
    case 'delivered': {
      until = now + 5000;
      const peer = (S?.peers || []).find(p => p.name === data.to);
      head(t('toast.delivered', { name: data.title }), t('toast.delivered_to', { to: data.to }));
      acts([]);
      bot.delivered(peer ? colorOf(peer.id) : '#60cdff');
      break;
    }
    case 'joined':
      until = now + (data.files ? 15000 : 6000);
      head(t('toast.joined', { name: data.name }), data.files
        ? `${t('share.added_by', { name: data.added_by })} ${t('share.question', { name: data.name, n: data.files })}` : t('toast.joined_text'));
      acts(data.files ? [
        [t('share.allow'), () => answerHistory(data.id, true), 'primary'],
        [t('share.deny'), () => answerHistory(data.id, false)],
      ] : []);
      bot.hello();
      break;
    case 'note': {
      until = now + 15000;
      head(t('note.from', { from: data.from }), esc(data.text));
      const list = [[`${icon('copy')}${t('btn.copy')}`, () => copyNote(data), 'primary']];
      if (data.url) list.push([`${icon('link')}${t('btn.open_link')}`, () => invoke('open_url', { url: data.url })]);
      acts(list);
      bot.receive({ note: true });
      break;
    }
  }
  renderOffers();
}

// Скопировать пришедшее сообщение прямо из шторки.
async function copyNote(data) {
  try {
    await invoke('copy_text', { text: data.text });
  } catch (e) {
    return head(esc(String(e)), '');
  }
  invoke('notes_seen', { ids: [data.id] }).catch(() => {});
  head(t('note.copied'), '');
  acts([]);
  bot.done();
  until = Date.now() + 2200;
}

// Экскурсия: Папыч сам глотает «файл» — всем, потом мини-робот устройства — только ему.
function demoDrag() {
  const r = isl.getBoundingClientRect();
  demoTimers.push(setTimeout(() => {
    bot.swallow({ name: 'фото.jpg' }, { from: [r.right + 60, r.top + 30], launch: '#60cdff' });
  }, 900));
  const m = members[0];
  if (!m) return;
  demoTimers.push(setTimeout(() => {
    const mr = m.el.getBoundingClientRect();
    members.forEach(x => x.el.classList.toggle('hot', x === m));
    $('#bot').classList.remove('hot');
    head(t('island.only', { name: m.name }), t('island.drop_hint'));
    m.p.swallow({ name: 'видео.mp4' }, { from: [mr.right + 90, mr.top - 10], launch: colorOf(m.id) });
  }, 4200));
}

function close() {
  demoTimers.forEach(clearTimeout);
  demoTimers = [];
  demo = false;
  if (!mode) return;
  mode = null;
  invoke('island_drop_bounds', { x: 0, y: 0, width: 0, height: 0 });
  dropBounds = '';
  isl.classList.remove('in');
  setTimeout(() => {
    if (mode) return;
    members.forEach(m => m.p.destroy());
    members = [];
    Papych.active(false);
    invoke('island_close');
  }, 230);
}

function renderPeek() {
  const peers = S?.peers || [];
  // Метки семьи: цвет — как глаза мини-робота устройства.
  $('#chips').innerHTML = peers.slice(0, 5).map(p =>
    `<span class="chip-peer ${p.online ? '' : 'off'}" style="color:${colorOf(p.id)}"><i style="background:${colorOf(p.id)}"></i><span style="color:#d6d8e2">${esc(p.name)}</span></span>`).join('')
    + (peers.length > 5 ? `<span class="chip-peer">+${peers.length - 5}</span>` : '');
  const moving = [...(S?.incoming || []), ...(S?.outgoing || [])].filter(i => i.state === 'downloading' && i.total);
  const total = moving.reduce((a, i) => a + i.total, 0);
  $('#track').classList.toggle('on', moving.length > 0);
  $('#track i').style.width = moving.length ? `${(moving.reduce((a, i) => a + i.done, 0) * 100) / total}%` : '0';
  const on = peers.filter(p => p.online).length;
  let title = !peers.length ? t('peer.none')
    : on === peers.length ? t('island.all_online')
    : on ? t('island.online', { n: on, m: peers.length }) : t('island.none_online');
  if (S && S.paused_until > Date.now()) title = t('island.paused');
  head(title, activity());
  acts([
    [icon('folderOpen'), () => invoke('open_folder'), 'icon', t('btn.open_folder')],
    [icon('app'), () => { invoke('show_main'); close(); }, 'icon', t('island.open_window')],
  ]);
}

function activity() {
  const moving = [...(S?.incoming || []), ...(S?.outgoing || [])].find(i => i.state === 'downloading' && i.total);
  if (moving) return `${esc(moving.item)} — ${Math.floor((moving.done * 100) / moving.total)}%`;
  const fresh = (S?.incoming || []).filter(i => i.state === 'new').length;
  if (fresh) return t('island.new_count', { n: fresh });
  return t('island.drag_hint');
}

function renderDrag() {
  const peers = S?.peers || [];
  const fam = $('#family');
  for (const p of peers) {
    const el = document.createElement('div');
    el.className = 'member' + (p.online ? '' : ' off');
    el.title = p.name;
    el.setAttribute('aria-label', p.name);
    const mb = document.createElement('div');
    mb.className = 'mb';
    const name = document.createElement('span');
    name.textContent = p.name;
    el.append(mb, name);
    fam.append(el);
    members.push({ id: p.id, name: p.name, el, p: new Papych(mb, { variant: 'robot', crop: true, hue: hueOf(p.id), interactive: false }) });
  }
  aim(-1, -1);
  acts([]);
}

// Куда целятся: на мини-Папыча — только ему, иначе всем.
function aim(x, y) {
  const hit = members.find(m => {
    const r = m.el.getBoundingClientRect();
    return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
  });
  target = hit ? hit.id : null;
  members.forEach(m => {
    m.el.classList.toggle('hot', m === hit);
    if (m === hit) m.p.dragOver(x, y); else m.p.dragEnd();
  });
  $('#bot').classList.toggle('hot', !hit && x >= 0);
  if (hit) bot.dragEnd(); else if (x >= 0) bot.dragOver(x, y);
  if (dropped) return;
  if (!members.length) head(t('island.to_all'), t('island.no_peers'));
  else if (hit) head(t('island.only', { name: hit.name }), t('island.drop_hint'));
  else head(t('island.to_all'), t('island.drop_hint'));
}

async function drop(paths, x, y) {
  if (!paths || !paths.length) return close();
  const r = isl.getBoundingClientRect();
  if (x < r.left || x > r.right || y < r.top || y > r.bottom) return close();
  if (mode !== 'drag') open('drag');
  aim(x, y); // выбор по месту отпускания, а не по последнему событию движения
  dropped = true;
  const hit = members.find(m => m.id === target);
  const who = hit ? hit.p : bot;
  const n = await invoke('send_dropped', { paths, peers: hit ? [hit.id] : [] }).catch(() => 0);
  setTimeout(() => { if (mode === 'drag') close(); }, 8000);   // на случай, если сценку перебили
  if (!n) {
    head(t('island.already'), '');
    bot.error();
    setTimeout(close, 2200);
    return;
  }
  members.forEach(m => { if (m !== hit) m.p.dragEnd(); });
  if (hit) bot.dragEnd();
  head(hit ? t('island.sent_one', { name: hit.name }) : t('island.sent_all'), '');
  await who.swallow({ name: paths.length === 1 ? baseName(paths[0]) : '', count: paths.length },
    { from: [x, y], launch: hit ? colorOf(hit.id) : '#60cdff' });
  close();
}

async function answerHistory(peer, allow) {
  try {
    await invoke('share_history', { peer, allow });
    close();
  } catch (e) { head(esc(String(e)), ''); }
}

// ---------- новые файлы ----------

function addOffer(o) {
  if (o && !offers.some(x => x.id === o.id)) offers.push(o);
}

function renderOffer() {
  const o = offers[offers.length - 1];
  if (!o) return close();
  const detail = [esc(o.title), esc(o.detail), o.has_exe ? t('toast.is_program') : ''].filter(Boolean).join(' · ');
  head(t(o.is_update ? 'toast.new_version' : 'toast.new_file', { from: o.from }), detail);
  acts([[t('btn.accept'), () => answer(o, true), 'primary'], [t('btn.decline'), () => answer(o, false)]]);
  renderOffers();
}

// Остальные ждущие файлы — строками под шторкой.
function renderOffers() {
  const el = $('#offers');
  el.innerHTML = '';
  if (mode !== 'offer') return;
  const rest = offers.slice(0, -1).reverse();
  for (const o of rest.slice(0, 2)) {
    const row = document.createElement('div');
    row.className = 'offer';
    row.innerHTML = `<div class="name">${esc(o.from)}: ${esc(o.title)}</div>`;
    for (const [label, ok, cls] of [[t('btn.accept'), true, 'primary'], [t('btn.decline'), false, '']]) {
      const b = document.createElement('button');
      b.className = cls;
      b.textContent = label;
      b.onclick = () => answer(o, ok);
      row.append(b);
    }
    el.append(row);
  }
  if (rest.length > 2) el.insertAdjacentHTML('beforeend', `<div class="more">${t('island.more', { n: rest.length - 2 })}</div>`);
}

function answer(o, ok) {
  offers = offers.filter(x => x.id !== o.id);
  invoke(ok ? 'accept' : 'decline', { id: o.id }).catch(e => head(esc(String(e)), ''));
  until = Date.now() + 12000;
  if (offers.length) return renderOffer();
  head(ok ? t('island.downloading') : t('island.declined'), '');
  acts([]);
  renderOffers();
  if (ok) bot.done();
  until = Date.now() + (ok ? 1800 : 700);
}

// ---------- состояние программы ----------

function onState(st) {
  const langChanged = S && st.lang !== S.lang;
  S = st;
  if (!S.settings.island && mode) close();
  if (langChanged) invoke('get_strings').then(s => { L = s; if (mode === 'peek') renderPeek(); });
  // Ответили в окне или приняли сами — убрать из шторки.
  const before = offers.length;
  offers = offers.filter(o => S.incoming.some(i => i.id === o.id && i.state === 'new'));
  if (mode === 'offer' && offers.length !== before) {
    if (offers.length) renderOffer(); else close();
  }
  if (mode === 'peek') renderPeek();
  isl.classList.toggle('busy', [...S.incoming, ...S.outgoing].some(i => i.state === 'downloading'));
  const paused = S.paused_until > Date.now();
  const offline = S.peers.length > 0 && !S.peers.some(p => p.online);
  if (bot.state.paused !== paused) bot.pause(paused);
  if (bot.state.offline !== offline) bot.offline(offline);
}

// Курсор над шторкой — кнопки нажимаются; мимо — мышь проходит к окнам под ней.
function setPass(p) {
  if (p === pass) return;
  pass = p;
  invoke('island_pass', { pass: p });
}

setInterval(() => {
  if (!mode) return;
  if (demo) return setPass(false);
  const r = isl.getBoundingClientRect();
  const bounds = [r.left, Math.max(0, r.top), r.width, Math.min(r.bottom, innerHeight) - Math.max(0, r.top)];
  const signature = bounds.map(Math.round).join(',');
  if (signature !== dropBounds) {
    dropBounds = signature;
    invoke('island_drop_bounds', { x: bounds[0], y: bounds[1], width: bounds[2], height: bounds[3] });
  }
  const inside = cursor.x >= r.left - 12 && cursor.x <= r.right + 12 && cursor.y <= r.bottom + 12;
  const now = Date.now();
  if (mode === 'drag') {
    setPass(false);
    if (dropped) return;
    if (!cursor.down) {
      releasedAt = releasedAt || now;
      if (now - releasedAt > 600) close();     // отпустили мимо или тащили не файлы
    } else {
      releasedAt = 0;
      if (cursor.y > r.bottom + 160 || Math.abs(cursor.x - innerWidth / 2) > 430) close();
    }
    return;
  }
  setPass(!inside);
  if (inside) {
    leftAt = 0;
    if (until) until = Math.max(until, now + 2500);
    return;
  }
  if (mode === 'peek') {
    leftAt = leftAt || now;
    if (now - leftAt > 450) close();
  } else if (until && now > until) {
    close();
  }
}, 100);

function onDrag(e) {
  const p = e.payload, k = devicePixelRatio || 1;
  const x = p.position ? p.position.x / k : cursor.x;
  const y = p.position ? p.position.y / k : cursor.y;
  if (p.type === 'enter' || p.type === 'over') {
    if (mode !== 'drag') open('drag');
    cursor = { x, y, down: true };
    Papych.cursor(x, y);
    aim(x, y);
  } else if (p.type === 'drop') {
    drop(p.paths, x, y);
  } else if (p.type === 'leave' && !dropped) {
    aim(-1, -1);
  }
}

(async () => {
  L = await invoke('get_strings');
  S = await invoke('get_state');
  onState(S);
  await T.event.listen('state', e => onState(e.payload));
  await T.event.listen('island-open', e => open(e.payload.reason, e.payload.data));
  await T.event.listen('island-cursor', e => {
    const [x, y, down] = e.payload;
    cursor = { x, y, down };
    Papych.cursor(x, y);
  });
  T.webview.getCurrentWebview().onDragDropEvent(onDrag);
  await T.event.listen('island-drag', onDrag);
  for (const o of await invoke('island_ready')) open(o.reason, o.data);
})();
