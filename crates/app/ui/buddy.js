'use strict';
// Папыч в окне программы. Сидит справа на полке вкладок, следит глазами за курсором по всему экрану
// и разыгрывает события: выплёвывает новый файл или сообщение в список, сам прыгает на кнопку
// «Получить», радуется скачанному, пускает самолётик, когда файл или сообщение доставлены,
// глотает файлы, брошенные на окно, и сообщения, которые отправляют, показывает на животике общий
// ход загрузок и подсказывает в облачке. На время экскурсии (tour.html) уходит из окна.
// Пользуется общими T, t, L, S, icon, view, switchTab из app.js.

const Buddy = (() => {
  const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
  let bot = null, box = null, bubble = null;
  let visible = true, flying = false, away = false, prev = null;
  let sayTimer = 0, hoverTimer = 0;
  let chatPose = false, chatGo = null, chatBack = null, chatToken = 0, chatButton = null;
  const sleep = ms => new Promise(r => setTimeout(r, ms));

  function init() {
    box = document.getElementById('buddy');
    if (!box || !window.Papych) return;
    bot = new Papych(box.querySelector('.buddy-bot'), { variant: 'robot', crop: true });
    bubble = box.querySelector('.buddy-say');
    T.event.listen('cursor', e => Papych.cursor(e.payload[0], e.payload[1]));
    T.event.listen('papych-active', e => {
      visible = e.payload;
      Papych.active(visible);
      if (visible) setTimeout(() => tip(false), 1800);
    });
    T.event.listen('tour', e => setAway(e.payload));
    T.webview?.getCurrentWebview().onDragDropEvent(onDrag);
    box.addEventListener('pointerenter', () => { hoverTimer = setTimeout(() => tip(true), 1600); });
    box.addEventListener('pointerleave', () => clearTimeout(hoverTimer));
  }

  // Экскурсия: Папыч выпрыгнул из окна на экран — здесь его нет; вернулся — приземляется.
  function setAway(on) {
    away = on;
    box.classList.toggle('away', on);
    if (!on && bot) {
      bot.jump(0.5);
      speak(t('tour.back'), 3500);
    }
  }

  // ---------- облачко ----------

  function speak(html, ms = 4500) {
    if (!bubble || !visible || away || view !== 'main') return;
    bubble.innerHTML = html;
    bubble.classList.remove('hidden', 'out');
    clearTimeout(sayTimer);
    sayTimer = setTimeout(() => {
      bubble.classList.add('out');
      sayTimer = setTimeout(() => bubble.classList.add('hidden'), 220);
    }, ms);
  }

  // Подсказки о том, что умеет Папыч и программа: каждая не больше двух раз, сами — не чаще раза
  // в час (наведёшь мышку на Папыча — расскажет сразу). Выключаются в настройках.
  const TIPS = ['tip.drop', 'tip.island', 'tip.island_one', 'tip.peek', 'tip.message', 'tip.paste', 'tip.copy',
    'tip.press', 'tip.poke', 'tip.pause', 'tip.sleep', 'tip.sad', 'tip.belly', 'tip.tour', 'tip.auto', 'tip.notify'];
  function tip(force) {
    if (!bot || bot.busy || flying || away || view !== 'main' || !S || !S.settings.tips) return;
    let seen = {};
    try { seen = JSON.parse(localStorage.getItem('papych.tips') || '{}'); } catch { /* нет хранилища — не беда */ }
    const now = Date.now();
    if (!force && now - (seen._last || 0) < 3600e3) return;
    const pool = force ? TIPS : TIPS.filter(k => (seen[k] || 0) < 2);
    const key = pool[Math.floor(Math.random() * pool.length)];
    if (!key) return;
    seen[key] = (seen[key] || 0) + 1;
    seen._last = now;
    try { localStorage.setItem('papych.tips', JSON.stringify(seen)); } catch { /* не запомнили — покажем ещё раз */ }
    speak(t(key), 7000);
    bot.wave();
  }

  // ---------- состояние программы ----------

  function update(S) {
    if (!bot) return;
    const paused = S.paused_until > Date.now();
    const offline = S.peers.length > 0 && !S.peers.some(p => p.online);
    if (!chatPose && bot.state.paused !== paused) bot.pause(paused);
    if (!chatPose && bot.state.offline !== offline) bot.offline(offline);
    const moving = [...S.incoming, ...S.outgoing].filter(i => i.state === 'downloading' && i.total);
    const total = moving.reduce((a, i) => a + i.total, 0);
    bot.setProgress(moving.length ? moving.reduce((a, i) => a + i.done, 0) / total : null);
    const delivered = n => n.outgoing && n.to.length > 0 && n.to.every(r => r.delivered);
    const cur = {
      in: new Map(S.incoming.map(i => [i.id, i.state])),
      out: new Map(S.outgoing.map(i => [i.id, i.state])),
      notes: new Map(S.notes.map(n => [n.id, delivered(n)])),
    };
    if (prev && visible && !flying && !away && !chatPose) events(S);
    prev = cur;
  }

  function events(S) {
    const fresh = S.incoming.filter(i => !prev.in.has(i.id) && (i.state === 'new' || i.state === 'done'));
    const finished = S.incoming.filter(i => i.state === 'done' && prev.in.has(i.id) && prev.in.get(i.id) !== 'done');
    const delivered = S.outgoing.filter(i => i.state === 'delivered' && prev.out.has(i.id) && prev.out.get(i.id) !== 'delivered');
    const note = S.notes.find(n => !n.outgoing && !prev.notes.has(n.id));
    const noteDone = S.notes.find(n => n.outgoing && prev.notes.get(n.id) === false && n.to.every(r => r.delivered));
    if (note && tab !== 'chat') {
      spitTo({ id: note.id, kind: 'note' });
      speak(t('note.from', { from: note.peer }));
    } else if (fresh.length) {
      const it = fresh[0];
      spitTo(it);
      speak(it.state === 'new' ? t('toast.new_file', { from: it.peer }) : t('toast.received', { name: it.item }));
    } else if (finished.length) {
      bot.done();
      flash(finished[0].id);
      speak(t('toast.received', { name: finished[0].item }));
    } else if (delivered.length) {
      const d = delivered[0];
      bot.delivered(Papych.colorOf(d.peer_id));
      speak(t('toast.delivered_to', { to: d.peer }));
    } else if (noteDone && tab !== 'chat') {
      bot.delivered(noteDone.to.length === 1 ? Papych.colorOf(noteDone.to[0].id) : '#60cdff');
      speak(noteDone.to.length === 1 ? t('toast.delivered_to', { to: noteDone.to[0].name }) : t('note.delivered_all'));
    }
  }

  const cardOf = id => document.querySelector(`[data-key="${CSS.escape(id)}"]`);

  function flash(id) {
    const card = cardOf(id);
    if (!card) return;
    card.classList.remove('flash');
    void card.offsetWidth;
    card.classList.add('flash');
  }

  // Новый файл или сообщение вылетает изо рта Папыча и ложится в свою карточку (или на вкладку).
  function spitTo(it) {
    let target = it.kind === 'note' && tab === 'chat'
      ? document.querySelector(`[data-message="${CSS.escape(it.id)}"] .chat-text, [data-message="${CSS.escape(it.id)}"] .voice-play`)
      : cardOf(it.id)?.querySelector('.tile');
    if (!target || !target.offsetParent) target = document.querySelector('.tab[data-tab="in"]');
    if (!target || reduce) { flash(it.id); return; }
    bot.spit();
    const [mx, my] = bot.mouthPoint();
    const r = target.getBoundingClientRect(), tx = r.left + r.width / 2, ty = r.top + r.height / 2;
    const g = document.createElement('div');
    g.className = `ghost-file tile ${it.kind}`;
    g.innerHTML = icon(it.kind === 'folder' ? 'folder' : it.kind === 'note' ? 'chat' : it.kind);
    document.body.append(g);
    const cx = Math.min(mx, tx) - 30, cy = Math.min(my, ty) - 60;   // дугой: вверх и влево
    const frames = [];
    for (let i = 0; i <= 14; i++) {
      const k = i / 14, u = 1 - k;
      const x = u * u * mx + 2 * u * k * cx + k * k * tx, y = u * u * my + 2 * u * k * cy + k * k * ty;
      frames.push({ transform: `translate(${x - 20}px, ${y - 20}px) scale(${0.35 + 0.65 * k}) rotate(${-260 * u}deg)`, opacity: Math.min(1, k * 8) });
    }
    g.animate(frames, { duration: 720, delay: 140, easing: 'cubic-bezier(.3,.6,.35,1)', fill: 'both' })
      .finished.then(() => { g.remove(); flash(it.id); });
  }

  // ---------- сообщения ----------

  // Отправили сообщение: листок летит из окна в рот Папычу, а из него — самолётик.
  function noteSent(from, color) {
    if (!bot) return;
    speak(t('note.sent'));
    if (reduce) return bot.delivered(color);
    bot.swallow({ note: true }, { from, launch: color });
  }

  // Пишут сообщение — Папыч поглядывает, что там.
  function watch(el) {
    if (!bot || bot.busy) return;
    const r = el.getBoundingClientRect();
    bot.glance = { p: [r.left + r.width * 0.7, r.top + r.height / 2], until: bot.t + 1.5 };
  }

  function copied() {
    if (bot && visible && !flying && !away) bot.boop();
  }

  // ---------- кнопки ----------

  // Дуга полёта с высшей точкой выше обеих точек.
  function arc(x0, y0, x1, y1, lift, scale) {
    const frames = [];
    for (let i = 0; i <= 16; i++) {
      const k = i / 16, s = 1 + (scale - 1) * Math.sin(Math.PI * k);
      const x = x0 + (x1 - x0) * k, y = y0 + (y1 - y0) * k - 4 * lift * k * (1 - k);
      frames.push({ transform: `translate(${x}px, ${y}px) scale(${s})` });
    }
    return frames;
  }

  // «Получить»: Папыч выпрыгивает со своего места, приземляется на кнопку и нажимает её.
  function press(btn, action) {
    if (!bot || !visible || flying || away || reduce || bot.state.paused || !btn.isConnected) return action();
    flying = true;
    const r0 = box.getBoundingClientRect(), r1 = btn.getBoundingClientRect();
    const dx = r1.left + r1.width / 2 - (r0.left + r0.width / 2);
    const dy = r1.top - r0.bottom + r0.height * 0.12;   // ногами на кнопку
    const lift = 60 + Math.abs(dy) * 0.25;
    bot.leap(true);
    const go = box.animate(arc(0, 0, dx, dy, lift, 0.82), { duration: 560, easing: 'cubic-bezier(.4,.1,.45,1)', fill: 'forwards' });
    go.finished.then(async () => {
      bot.leap(false);
      btn.classList.add('pressed');
      action();
      await sleep(420);
      btn.classList.remove('pressed');
      const back = box.animate(arc(dx, dy, 0, 0, lift, 0.82), { duration: 540, easing: 'cubic-bezier(.4,.1,.45,1)', fill: 'forwards' });
      await back.finished;
      go.cancel();
      back.cancel();
      bot.sp.squash.v += 6;
      flying = false;
    }).catch(() => { flying = false; action(); });
  }

  // «Отклонить» — пожимает плечами.
  function declined() {
    if (bot && visible && !flying && !away) bot.shrug();
  }

  // ---------- файлы, брошенные на окно ----------

  function onDrag(e) {
    const p = e.payload, k = devicePixelRatio || 1;
    const x = p.position ? p.position.x / k : 0, y = p.position ? p.position.y / k : 0;
    const zone = document.getElementById('drop-zone');
    if (p.type === 'enter' || p.type === 'over') {
      zone.classList.remove('hidden');
      Papych.cursor(x, y);
      if (!flying) bot?.dragOver(x, y);
    } else if (p.type === 'leave') {
      zone.classList.add('hidden');
      bot?.dragEnd();
    } else if (p.type === 'drop') {
      zone.classList.add('hidden');
      dropped(p.paths || [], x, y);
    }
  }

  async function dropped(paths, x, y) {
    if (!paths.length) return bot?.dragEnd();
    const n = await T.core.invoke('send_dropped', { paths, peers: [] }).catch(() => 0);
    if (!n) {
      bot?.dragEnd();
      bot?.error();
      speak(t('island.already'));
      return;
    }
    speak(S.peers.length ? t('island.sent_all') : t('island.no_peers'));
    if (view === 'main') switchTab('out');
    if (bot && !reduce) {
      const name = paths.length === 1 ? String(paths[0]).split(/[\\/]/).pop() : '';
      bot.swallow({ name, count: paths.length }, { from: [x, y], launch: '#60cdff' });
    }
  }

  // Один Папыч для вкладок файлов и чата: все прыжки начинаются с верхней полки.
  function chatVoice(mode, level = 0) {
    if (!bot) return;
    chatPose = true;
    box.classList.toggle('recording', mode === 'record');
    box.classList.toggle('speaking', mode === 'play');
    if (!chatGo || chatGo.playState === 'finished') bot.chatVoice(mode, level);
  }
  function chatVisit(button, mode) {
    if (!bot || !button?.isConnected) return;
    const token = ++chatToken;
    const matrix = new DOMMatrix(getComputedStyle(box).transform);
    chatGo?.cancel(); chatBack?.cancel(); chatGo = chatBack = null;
    chatButton?.classList.remove('pressed'); chatButton = button;
    chatVoice(mode);
    if (reduce) { button.classList.add('pressed'); return; }
    const home = box.getBoundingClientRect(), target = button.getBoundingClientRect();
    const dx = target.left + target.width / 2 - home.left - home.width / 2;
    const dy = target.top + target.height / 2 - home.bottom + home.height * .15;
    flying = true; box.classList.add('chat-visiting'); bot.leap(true);
    chatGo = box.animate(arc(matrix.m41, matrix.m42, dx, dy, 45 + Math.abs(dy) * .12, .85),
      { duration: 480, easing: 'cubic-bezier(.3,.1,.35,1)', fill: 'forwards' });
    chatGo.finished.then(() => {
      if (token !== chatToken) return;
      bot.leap(false); bot.chatVoice(mode, 0); chatButton?.classList.add('pressed');
    }).catch(() => {});
  }
  function chatTarget(button) {
    if (!chatPose || !button) return;
    const pressed = chatButton?.classList.contains('pressed');
    chatButton = button;
    if (pressed || chatGo?.playState === 'finished') button.classList.add('pressed');
  }
  function chatReturn() {
    if (!bot || !chatPose && !chatGo && !chatBack) return;
    const token = ++chatToken;
    const transform = getComputedStyle(box).transform;
    chatGo?.cancel(); chatBack?.cancel(); chatGo = chatBack = null;
    chatButton?.classList.remove('pressed'); chatButton = null;
    chatPose = false; box.classList.remove('recording', 'speaking'); bot.settle();
    if (reduce || transform === 'none') { flying = false; box.classList.remove('chat-visiting'); return; }
    flying = true;
    chatBack = box.animate([{ transform }, { transform: 'translate(0,0) scale(1)' }],
      { duration: 360, easing: 'cubic-bezier(.2,.7,.3,1)', fill: 'forwards' });
    chatBack.finished.then(() => {
      if (token !== chatToken) return;
      chatBack.cancel(); chatBack = null; flying = false;
      box.classList.remove('chat-visiting'); bot.sp.squash.v += 3;
    }).catch(() => {});
  }
  function chatReceived(n) {
    if (!chatPose && !flying) spitTo({ id: n.id, kind: 'note' });
  }

  return { init, update, press, declined, speak, noteSent, watch, copied, chatVoice, chatVisit, chatTarget, chatReturn, chatReceived, actor: () => bot, visible: () => visible && !away };
})();
