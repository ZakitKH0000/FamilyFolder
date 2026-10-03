'use strict';
// Экскурсия с Папычем по экрану. Окно на весь монитор (tour.rs); TOUR.main — где окно программы
// (в пикселях этой страницы). Папыч выпрыгивает из окна, ходит по экрану с указкой, показывает
// шторку (через island_demo), окно, сообщения, паузу у часов — и возвращается в окно.

const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const $ = sel => document.querySelector(sel);
let L = {};

function esc(s) {
  return String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}
function t(key, p) {
  const s = typeof L[key] === 'string' ? L[key] : key;
  return p ? s.replace(/\{(\w+)\}/g, (m, k) => (k in p ? esc(p[k]) : m)) : s;
}
const sleep = ms => new Promise(r => setTimeout(r, ms));
const W = () => innerWidth, H = () => innerHeight;

const geo = (window.TOUR && window.TOUR.main) || [innerWidth - 400, innerHeight - 680, 380, 620];
const main = { x: geo[0], y: geo[1], w: geo[2], h: geo[3] };

// Папыч: точка (x, y) — где его ноги. Рисунок 230×211, ноги — на 85,1 % высоты.
const box = $('#bot');
const bot = new Papych(box.querySelector('.svg'), { variant: 'robot' });
const BW = 230, FEET = 211.2 * 0.851;
const SMALL = 78 / 230;                                                    // размер, как в окне
const home = () => ({ x: main.x + main.w - 47, y: main.y + 116 });        // полка вкладок в окне
let cur = { ...home(), s: SMALL };

function place(p) {
  box.style.transform = `translate(${p.x - BW / 2}px, ${p.y - FEET}px) scale(${p.s})`;
}

// Прыжок по дуге в точку: подпрыгивает, летит, приземляется.
async function move(to, { s = 1, dur = 900, lift = 120 } = {}) {
  const from = { ...cur }, target = { ...to, s };
  const frames = [];
  for (let i = 0; i <= 20; i++) {
    const k = i / 20;
    const x = from.x + (target.x - from.x) * k;
    const y = from.y + (target.y - from.y) * k - 4 * lift * k * (1 - k);
    const sc = from.s + (target.s - from.s) * k;
    frames.push({ transform: `translate(${x - BW / 2}px, ${y - FEET}px) scale(${sc})` });
  }
  bot.leap(true);
  const a = box.animate(frames, { duration: dur, easing: 'cubic-bezier(.45,.05,.4,1)', fill: 'forwards' });
  await a.finished;
  cur = target;
  place(cur);
  a.cancel();
  bot.leap(false);
}

// Робот рядом с окном программы — слева, а если слева нет места — справа.
function beside(dy = 0.45) {
  const left = main.x - 150;
  return { x: left > 170 ? left : main.x + main.w + 150, y: main.y + main.h * dy };
}

function spot(s) {
  const dim = $('#dim');
  dim.classList.add('on');
  dim.style.setProperty('--sx', `${s ? s.x : W() / 2}px`);
  dim.style.setProperty('--sy', `${s ? s.y : H() / 2}px`);
  dim.style.setProperty('--sr', `${s ? s.r : 0}px`);
}

// Облачко рядом с Папычем: с той стороны, где больше места.
function bubble(step, n) {
  const say = $('#say');
  $('#step').textContent = t('tour.step', { n: n + 1, m: STEPS.length });
  $('#title').innerHTML = t(`tour.${step.key}.title`);
  $('#text').innerHTML = t(`tour.${step.key}.text`);
  $('#next').textContent = n === STEPS.length - 1 ? t('tour.done') : t('tour.next');
  $('#skip').style.visibility = n === STEPS.length - 1 ? 'hidden' : '';
  say.classList.remove('hidden', 'pop');
  void say.offsetWidth;
  say.classList.add('pop');
  const r = box.getBoundingClientRect();
  const sw = say.offsetWidth, sh = say.offsetHeight;
  // Сбоку — где больше места; под Папычем — когда указка смотрит вверх (облачко её не загородит).
  let x = r.left + r.width / 2 > W() / 2 ? r.left - sw + 10 : r.right - 10;
  let y = r.top + r.height * 0.1;
  if (step.side === 'below') {
    x = r.left + r.width / 2 - sw / 2;
    y = r.bottom - 6;
  }
  x = Math.max(16, Math.min(W() - sw - 16, x));
  y = Math.max(16, Math.min(H() - sh - 16, y));
  say.style.left = `${x}px`;
  say.style.top = `${y}px`;
}

const STEPS = [
  {
    key: 'hello', at: () => beside(), enter: () => bot.hello(),
  },
  {
    key: 'island', side: 'below', at: () => ({ x: W() / 2 - 330, y: 300 }), point: () => [W() / 2, 46],
    spot: () => ({ x: W() / 2, y: 30, r: 230 }), enter: () => invoke('island_demo', { kind: 'peek' }),
  },
  {
    key: 'drag', side: 'below', at: () => ({ x: W() / 2 - 330, y: 300 }), point: () => [W() / 2, 120],
    spot: () => ({ x: W() / 2, y: 100, r: 290 }), enter: () => invoke('island_demo', { kind: 'drag' }),
  },
  {
    key: 'window', at: () => beside(), point: () => [main.x + main.w / 2, main.y + main.h * 0.42],
    spot: () => ({ x: main.x + main.w / 2, y: main.y + main.h / 2, r: Math.max(main.w, main.h) / 2 + 10 }),
    enter: () => {
      invoke('island_demo', { kind: 'close' });
      setTimeout(() => bot.swallow({ name: 'отпуск.jpg' }, { from: [main.x + main.w / 2, main.y + main.h * 0.4], launch: '#60cdff' }), 700);
    },
  },
  {
    key: 'messages', at: () => beside(0.6), point: () => [main.x + 175, main.y + main.h - 26],
    spot: () => ({ x: main.x + 175, y: main.y + main.h - 26, r: 95 }), enter: () => bot.receive({ note: true }),
  },
  {
    key: 'tray', at: () => ({ x: W() - 270, y: H() - 120 }), point: () => [W() - 150, H() - 22],
    spot: () => ({ x: W() - 150, y: H() - 22, r: 150 }),
    enter: () => { bot.pause(true); setTimeout(() => bot.pause(false), 3200); },
  },
  {
    key: 'pet', at: () => ({ x: W() / 2, y: H() / 2 + 120 }),
    enter: () => { bot.boop(); setTimeout(() => bot.sneeze(), 1300); },
  },
  {
    key: 'bye', at: () => ({ x: W() / 2, y: H() / 2 + 120 }), enter: () => bot.update(),
  },
];

let n = -1, busy = false, ended = false;

async function go(i) {
  if (busy || ended) return;
  busy = true;
  n = i;
  const step = STEPS[i];
  $('#say').classList.add('hidden');
  bot.point(null);
  const at = step.at();
  if (Math.hypot(at.x - cur.x, at.y - cur.y) > 20 || cur.s !== 1) {
    await move(at, { s: 1, dur: i === 0 ? 1100 : 900, lift: i === 0 ? 220 : 110 });
  }
  if (step.spot) spot(step.spot()); else spot(null);
  if (step.point) {
    const [px, py] = step.point();
    bot.point(px, py);
    bot.glance = { p: [px, py], until: bot.t + 2.5 };
  }
  if (step.enter) step.enter();
  await sleep(250);
  bubble(step, i);
  busy = false;
}

// Конец: Папыч прыгает обратно в окно программы.
async function finish() {
  if (ended) return;
  ended = true;
  $('#say').classList.add('hidden');
  $('#dim').classList.remove('on');
  bot.point(null);
  invoke('island_demo', { kind: 'close' }).catch(() => {});
  await sleep(150);
  await move(home(), { s: SMALL, dur: 1000, lift: 160 });
  box.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 200, fill: 'forwards' });
  await sleep(200);
  invoke('tour_done').catch(() => {});
}

$('#next').onclick = () => (n >= STEPS.length - 1 ? finish() : go(n + 1));
$('#skip').onclick = () => finish();
addEventListener('keydown', e => {
  if (e.key === 'Escape') finish();
  else if (e.key === 'Enter' || e.key === 'ArrowRight') $('#next').click();
});

(async () => {
  L = await invoke('get_strings');
  place(cur);
  await sleep(500);
  bot.expr('excited');
  bot.sp.squash.v += 6;
  await sleep(250);
  go(0);
})();
