// Папыч — живая папка: персонаж шторки и окна «Общей папки».
// Рисуется кодом (SVG), без картинок: чёткий на любом размере и в любом цвете.
// Смотрит за курсором, дышит, моргает, скучает и засыпает, разыгрывает сценки на события программы.
// Облики: classic — жёлтая папка, plush — мягкая игрушка, robot — робо-папка с экраном.
(function () {
  'use strict';
  const NS = 'http://www.w3.org/2000/svg';
  const VB = [-110, -172, 220, 202];   // всё поле: место для прыжков, колпака и эффектов
  const CROP = [-80, -128, 160, 138];  // только сам Папыч — для маленьких размеров
  const W = 124, HF = 84;              // ширина тела и высота клапана-лица (задняя стенка — 104)
  const TAU = Math.PI * 2;

  const clamp = (v, a = 0, b = 1) => (v < a ? a : v > b ? b : v);
  const lerp = (a, b, t) => a + (b - a) * t;
  const smooth = (a, b, v) => { const t = clamp((v - a) / (b - a)); return t * t * (3 - 2 * t); };
  const rnd = (a, b) => a + Math.random() * (b - a);
  const r2 = v => Math.round(v * 100) / 100;
  const hsl = (h, s, l, a = 1) => `hsla(${h},${s}%,${l}%,${a})`;
  const now = () => performance.now() / 1000;
  const ease = {
    out: t => 1 - (1 - t) ** 3,
    in: t => t * t * t,
    inOut: t => (t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2),
    back: t => { const c = 1.8; return 1 + (c + 1) * (t - 1) ** 3 + c * (t - 1) ** 2; },
  };
  const bez = (a, b, c, d, t) => { const u = 1 - t; return [0, 1].map(i => u * u * u * a[i] + 3 * u * u * t * b[i] + 3 * u * t * t * c[i] + t * t * t * d[i]); };
  const FONT = 'Segoe UI Variable Display, Segoe UI, system-ui, sans-serif';
  const PAGE = 'M -15 -21 L 6 -21 L 15 -12 L 15 21 L -15 21 Z';
  const STAR = 'M 0 -9 Q 1.6 -1.6 9 0 Q 1.6 1.6 0 9 Q -1.6 1.6 -9 0 Q -1.6 -1.6 0 -9 Z';

  function el(tag, attrs, parent) {
    const e = document.createElementNS(NS, tag);
    if (attrs) for (const k in attrs) e.setAttribute(k, attrs[k]);
    if (parent) parent.appendChild(e);
    return e;
  }
  const set = (e, k, v) => e.setAttribute(k, v);

  const STYLES = {
    classic: { hue: 40, sat: 92, r: 11, eyes: 'cartoon', a: 12.5, b: 14.5, ex: 22, ey: -52, my: -27, limb: 7.5, line: 0, mouth: '#5b2316' },
    plush: { hue: 212, sat: 80, r: 22, eyes: 'kawaii', a: 10.5, b: 12.8, ex: 23, ey: -51, my: -28, limb: 9.5, line: 2.4, mouth: '#33244f' },
    robot: { hue: 188, sat: 100, r: 9, eyes: 'led', a: 6.5, b: 11, ex: 20, ey: -50, my: -29, limb: 6, line: 0, mouth: null },
  };

  function palette(st, h) {
    if (st.eyes === 'led') return {
      back1: '#59647a', back2: '#2a303b', front1: '#6f7a8e', front2: '#434c5c', rim: 'rgba(255,255,255,.4)',
      line: '#232832', limb: '#3a414e', hand: '#8a95a8', inside: '#161a22', cap: hsl((h + 170) % 360, 70, 58),
      led: hsl(h, 100, 64), glow: hsl(h, 100, 60, 0.25), cheek: [hsl(335, 100, 66, 0.9), hsl(335, 100, 66, 0)],
    };
    const s = st.sat;
    return {
      back1: hsl(h - 4, s, 56), back2: hsl(h - 9, s, 40), front1: hsl(h + 2, s, 70), front2: hsl(h - 2, s, 57),
      rim: hsl(h + 6, 100, 88), line: hsl(h - 12, Math.min(70, s), 28), limb: hsl(h - 8, s * 0.85, 42),
      hand: hsl(h, s, 64), inside: hsl(h - 12, s * 0.7, 24), cap: hsl((h + 190) % 360, 65, 55),
      cheek: [hsl(352, 95, 68, 0.95), hsl(352, 95, 68, 0)],
    };
  }

  // Выражения лица: всё, что не указано, берётся из BASE.
  const BASE = { lidTop: 0.06, lidBot: 0, lidTilt: 0, eyeS: 1, happyEyes: 0, sleepEyes: 0, spiralEyes: 0, heartEyes: 0,
    brA: 0, brY: 0, brT: 0, brAsym: 0, mW: 7, mC: 3, mO: 0, wob: 0, blush: 0.45 };
  const EXPR = {
    neutral: {},
    happy: { lidBot: 0.38, mW: 10, mC: 6, mO: 5, blush: 0.85 },
    joy: { happyEyes: 1, mW: 11, mC: 7, mO: 10, blush: 1 },
    excited: { lidTop: 0, eyeS: 1.13, brA: 0.7, brY: 5, mW: 8, mC: 3, mO: 9, blush: 0.75 },
    surprised: { lidTop: 0, eyeS: 1.18, brA: 1, brY: 8, mW: 4, mC: 0, mO: 7 },
    sad: { lidTop: 0.32, lidTilt: -0.8, brA: 1, brT: -1, mW: 7, mC: -3.5, blush: 0.15 },
    worried: { lidTop: 0.14, lidTilt: -0.5, brA: 1, brT: -1, brY: 3, mW: 7, mC: -1, wob: 2.4, blush: 0.3 },
    focused: { lidTop: 0.26, lidTilt: 0.2, brA: 0.6, brT: 0.45, mW: 5, mC: 0.5 },
    confused: { lidTop: 0.12, brA: 1, brAsym: 1, brT: -0.3, mW: 6, mC: -1, wob: 2.2 },
    sleepy: { lidTop: 0.62, mW: 5, mC: 1, blush: 0.5 },
    sleep: { sleepEyes: 1, mW: 4, mC: 1, mO: 2.5, blush: 0.6 },
    strain: { lidTop: 0.36, lidTilt: -0.45, brA: 1, brT: -0.8, mW: 9, mC: -2, mO: 3, wob: 1.6, blush: 0.8 },
    dizzy: { spiralEyes: 1, mW: 8, mC: 0, mO: 3, wob: 3 },
    love: { heartEyes: 1, mW: 9, mC: 6, mO: 5, blush: 1 },
    yawn: { lidTop: 0.78, mW: 7, mC: 0, mO: 17, blush: 0.5 },
    shy: { lidBot: 0.3, lidTop: 0.1, mW: 6, mC: 2.5, wob: 1.2, blush: 1.3 },
    squint: { lidTop: 0.55, lidBot: 0.35, lidTilt: 0.2, mW: 7, mC: -1, mO: 4 },
    sneeze: { happyEyes: 1, mW: 9, mC: -1, mO: 9, blush: 0.9 },
  };

  function backPath(r) {
    const x = W / 2, t = -104;
    return `M ${-x} ${-r} L ${-x} -110 Q ${-x} -116 ${-x + 6} -116 L -26 -116 Q -20 -116 -17 -112 L -11 -106 Q -9 ${t} -5 ${t} ` +
      `L ${x - r} ${t} Q ${x} ${t} ${x} ${t + r} L ${x} ${-r} Q ${x} 0 ${x - r} 0 L ${-x + r} 0 Q ${-x} 0 ${-x} ${-r} Z`;
  }
  // Клапан наклоняется к зрителю: верхний край опускается и чуть расширяется — это «рот».
  function flapPath(r, open, ins = 0) {
    const x = W / 2 + 2 - ins, e = open * 6, yt = -HF + open * 30 + ins, yb = -ins;
    const rt = Math.max(2, Math.min(r, 16) - ins * 0.6), rb = Math.max(2, r - ins * 0.6);
    return `M ${r2(-x - e + rt)} ${r2(yt)} L ${r2(x + e - rt)} ${r2(yt)} Q ${r2(x + e)} ${r2(yt)} ${r2(x + e)} ${r2(yt + rt)} ` +
      `L ${x} ${r2(yb - rb)} Q ${x} ${yb} ${r2(x - rb)} ${yb} L ${r2(-x + rb)} ${yb} Q ${-x} ${yb} ${-x} ${r2(yb - rb)} ` +
      `L ${r2(-x - e)} ${r2(yt + rt)} Q ${r2(-x - e)} ${r2(yt)} ${r2(-x - e + rt)} ${r2(yt)} Z`;
  }
  function mouthPath(w, c, o, wob) {
    const c1 = c * 1.33, o1 = (c + o) * 1.33;
    return `M ${r2(-w)} 0 C ${r2(-w / 3)} ${r2(c1 - wob)} ${r2(w / 3)} ${r2(c1 + wob)} ${r2(w)} 0 ` +
      `C ${r2(w / 3)} ${r2(o1 + wob)} ${r2(-w / 3)} ${r2(o1 - wob)} ${r2(-w)} 0 Z`;
  }
  function spiralPath(R) {
    let d = '';
    for (let i = 0; i <= 40; i++) { const a = i * 0.42, r = (R * i) / 40; d += `${i ? 'L' : 'M'} ${r2(Math.cos(a) * r)} ${r2(Math.sin(a) * r)} `; }
    return d;
  }
  function heartPath(s) {
    return `M 0 ${r2(s * 0.75)} C ${r2(-s * 1.25)} ${r2(-s * 0.1)} ${r2(-s * 0.6)} ${r2(-s * 1.05)} 0 ${r2(-s * 0.4)} ` +
      `C ${r2(s * 0.6)} ${r2(-s * 1.05)} ${r2(s * 1.25)} ${r2(-s * 0.1)} 0 ${r2(s * 0.75)} Z`;
  }
  function extColor(ext) {
    const e = ext.toLowerCase();
    if (/^(jpe?g|png|gif|heic|webp|bmp|raw)$/.test(e)) return '#12a37a';
    if (/^(mp4|mov|avi|mkv|webm)$/.test(e)) return '#7b4ee8';
    if (/^(mp3|wav|flac|ogg|m4a)$/.test(e)) return '#e2477c';
    if (e === 'pdf') return '#e03b3b';
    if (/^(docx?|txt|rtf|odt)$/.test(e)) return '#2b6fd6';
    if (/^(xlsx?|csv)$/.test(e)) return '#1f8f4e';
    if (/^(zip|rar|7z|tar|gz)$/.test(e)) return '#e58a00';
    if (/^(exe|msi)$/.test(e)) return '#5b6573';
    return '#8a93a0';
  }

  const all = new Set();
  const input = { x: null, y: null, t: now() };
  let uid = 0;

  class Papych {
    constructor(host, opt = {}) {
      this.host = host;
      this.variant = STYLES[opt.variant] ? opt.variant : 'classic';
      this.st = STYLES[this.variant];
      this.hue = opt.hue ?? this.st.hue;
      this.crop = !!opt.crop;
      this.interactive = opt.interactive !== false;
      this.id = ++uid;
      this.t = 0; this.tok = 0; this.busy = false; this.scene = null;
      this.state = { paused: false, offline: false };
      this.timers = []; this.tweens = []; this.parts = [];
      this.jy = 0; this.jv = 0; this.breath = rnd(0, TAU); this.blink = 0; this.blinkAge = -1; this.blinkIn = rnd(1, 4);
      this.look = null; this.glance = null; this.wander = null; this.wanderIn = 0;
      this.waveT = 0; this.wave2 = false; this.lastWave = -9; this.hovered = false; this.clicks = [];
      this.sleeping = false; this.autoSleep = false; this.yawned = false; this.zIn = 0; this.shake = 0; this.glintX = -200; this.lastSat = 1;
      this.progress = null;   // ход передачи на животике (0…1), null — полоски нет
      this.sp = {};
      const sp = (k, x, stiff = 150, z = 0.8) => { this.sp[k] = { x, v: 0, to: x, k: stiff, d: 2 * Math.sqrt(stiff) * z }; };
      for (const k in BASE) sp(k, BASE[k], 150, 0.85);
      for (const k of ['elx', 'ely', 'erx', 'ery']) sp(k, 0, 600, 0.9);   // взгляд каждого глаза (−1…1)
      sp('fx', 0, 90, 0.9); sp('fy', 0, 90, 0.9);                         // поворот «головы» к курсору
      sp('open', 0, 280, 0.5); sp('squash', 0, 330, 0.3); sp('lean', 0, 120, 0.45); sp('paper', 0, 200, 0.35);
      sp('hlx', -68, 170, 0.6); sp('hly', -12, 170, 0.6); sp('hrx', 68, 170, 0.6); sp('hry', -12, 170, 0.6);
      sp('cap', 0, 160, 0.45); sp('prog', 0, 60, 1); sp('progA', 0, 120, 0.8); sp('sat', 1, 60, 1);
      sp('offA', 0, 120, 0.7); sp('sweat', 0, 100, 1); sp('spin', 0, 80, 0.7); sp('stickA', 0, 140, 0.8);
      this.pointAt = null; this.stickAng = 0; this.stickSide = 1;   // указка: куда показывает (экран)
      this.pal = palette(this.st, this.hue);
      this.build();
      this.expr('neutral');
      all.add(this);
      start();
    }

    setHue(h) {
      this.hue = h ?? this.st.hue;
      this.pal = palette(this.st, this.hue);
      this.build();
    }

    destroy() { all.delete(this); this.svg.remove(); }

    // ---------- рисунок ----------
    build() {
      const st = this.st, P = this.pal, led = st.eyes === 'led', id = s => `pp${this.id}${s}`;
      const vb = (this.vb = this.crop ? CROP : VB);
      const svg = el('svg', { viewBox: vb.join(' '), class: 'papych papych-' + this.variant, 'aria-hidden': 'true' });
      svg.style.overflow = 'visible';
      svg.style.display = 'block';
      if (this.svg) this.svg.replaceWith(svg); else this.host.appendChild(svg);
      this.svg = svg; this.parts = []; this.lastSat = 1;
      const defs = el('defs', null, svg);
      const lin = (k, y1, y2, c1, c2) => {
        const g = el('linearGradient', { id: id(k), gradientUnits: 'userSpaceOnUse', x1: 0, y1, x2: 0, y2 }, defs);
        el('stop', { offset: 0, 'stop-color': c1 }, g); el('stop', { offset: 1, 'stop-color': c2 }, g);
        return `url(#${id(k)})`;
      };
      const rad = (k, c1, c2) => {
        const g = el('radialGradient', { id: id(k) }, defs);
        el('stop', { offset: 0, 'stop-color': c1 }, g); el('stop', { offset: 1, 'stop-color': c2 }, g);
        return `url(#${id(k)})`;
      };
      const backF = lin('b', -116, 0, P.back1, P.back2);
      const frontF = lin('f', -HF, 0, P.front1, P.front2);
      const lidF = lin('l', -HF - st.ey, -st.ey, P.front1, P.front2);   // веки — цвета лица на высоте глаз
      const deepF = lin('d', -100, -36, 'rgba(0,0,0,0)', P.inside);
      const outline = st.line ? { stroke: P.line, 'stroke-width': st.line, 'stroke-linejoin': 'round' } : {};
      const n = (this.n = {});

      n.shadow = el('ellipse', { cx: 0, cy: 3, rx: 62, ry: 9, fill: rad('s', 'rgba(15,20,30,.3)', 'rgba(15,20,30,0)') }, svg);
      const root = (n.root = el('g', null, svg));
      if (this.interactive) root.style.cursor = 'pointer';
      n.feet = [-1, 1].map(() => el('ellipse', { rx: led ? 10 : 11.5, ry: led ? 5 : 6, fill: P.limb, ...outline }, root));

      // Задняя стенка с ушком. Видна её изнанка: внутри папки темнее.
      n.back = el('g', null, root);
      el('path', { d: backPath(st.r), fill: backF, ...outline }, n.back);
      el('path', { d: 'M -54 -112.5 L -27 -112.5', stroke: P.rim, 'stroke-width': 2, 'stroke-linecap': 'round', opacity: 0.6 }, n.back);
      el('rect', { x: -58, y: -100, width: 116, height: 98, rx: 8, fill: deepF }, n.back);
      if (this.variant === 'plush') {
        n.curl = el('g', null, n.back);
        el('path', { d: 'M -40 -115 C -43 -129 -29 -134 -30 -125 C -31 -119 -38 -121 -36 -127', fill: 'none', stroke: P.line, 'stroke-width': 2.6, 'stroke-linecap': 'round' }, n.curl);
      }
      if (led) {
        n.ant = el('g', null, n.back);
        el('line', { x1: -40, y1: -116, x2: -40, y2: -133, stroke: P.limb, 'stroke-width': 3, 'stroke-linecap': 'round' }, n.ant);
        el('circle', { cx: -40, cy: -137, r: 7.5, fill: P.glow }, n.ant);
        n.antBall = el('circle', { cx: -40, cy: -137, r: 4.2, fill: P.led }, n.ant);
        for (const y of [-97, -92]) el('line', { x1: 30, x2: 50, y1: y, y2: y, stroke: 'rgba(0,0,0,.35)', 'stroke-width': 2, 'stroke-linecap': 'round' }, n.back);
      }

      // Листы внутри: два с текстом и фотография.
      n.papers = el('g', null, root);
      n.sheets = [
        { x: -30, y: -98, r: -7, w: 50, c: '#fffaf0' },
        { x: 10, y: -101, r: 6, w: 50, c: '#ffffff' },
        { x: -6, y: -95, r: 2, w: 44, c: null },
      ].map(s => {
        const g = el('g', null, n.papers);
        el('rect', { x: -s.w / 2, y: 0, width: s.w, height: 92, rx: 2.5, fill: s.c || '#f4f8ff', stroke: 'rgba(0,0,0,.13)', 'stroke-width': 0.8 }, g);
        if (s.c) {
          for (let j = 0; j < 4; j++) el('line', { x1: -s.w / 2 + 6, x2: s.w / 2 - 6 - (j % 2) * 10, y1: 7 + j * 5.5, y2: 7 + j * 5.5, stroke: '#cfd6e0', 'stroke-width': 1.5, 'stroke-linecap': 'round' }, g);
        } else {
          el('rect', { x: -s.w / 2 + 4, y: 4, width: s.w - 8, height: 30, rx: 1.5, fill: '#bfe0ff' }, g);
          el('circle', { cx: 10, cy: 11, r: 3.6, fill: '#ffd43b' }, g);
          el('path', { d: `M ${-s.w / 2 + 4} 34 L -8 16 L 0 25 L 8 15 L ${s.w / 2 - 4} 34 Z`, fill: '#5cb85c' }, g);
        }
        return { g, ...s, ph: rnd(0, TAU) };
      });
      n.slotIn = el('g', null, root);       // то, что проваливается внутрь (за клапан)

      // Клапан — лицо.
      n.flap = el('g', null, root);
      n.flapPath = el('path', { fill: frontF, ...outline }, n.flap);
      const cf = el('clipPath', { id: id('cf') }, defs);
      n.flapClip = el('path', null, cf);
      if (this.variant === 'plush') n.seam = el('path', { fill: 'none', stroke: 'rgba(255,255,255,.65)', 'stroke-width': 1.6, 'stroke-dasharray': '3.5 4', 'stroke-linecap': 'round' }, n.flap);
      n.rim = el('path', { fill: 'none', stroke: P.rim, 'stroke-width': 2.4, 'stroke-linecap': 'round', opacity: 0.85 }, n.flap);
      el('path', { d: `M ${-W / 2} -7 L ${W / 2} -7`, stroke: 'rgba(0,0,0,.08)', 'stroke-width': 1.5, 'clip-path': `url(#${id('cf')})` }, n.flap);
      if (led) {
        n.screen = el('g', null, n.flap);
        el('rect', { x: -44, y: -78, width: 88, height: 60, rx: 13, fill: '#0b0f15', stroke: '#272e3a', 'stroke-width': 2.5 }, n.screen);
        el('path', { d: 'M -38 -75 L -16 -75 L -34 -21 L -41 -21 L -41 -68 Z', fill: 'rgba(255,255,255,.05)' }, n.screen);
      }
      n.face = el('g', null, n.flap);
      const cheekF = rad('c', P.cheek[0], P.cheek[1]);
      n.cheeks = [-1, 1].map(s => el('ellipse', { cx: s * (led ? 31 : 41), cy: led ? -33 : -34, rx: led ? 6 : 9, ry: led ? 3.4 : 5.5, fill: cheekF }, n.face));
      const ce = el('clipPath', { id: id('ce') }, defs);
      el('ellipse', { rx: st.a, ry: st.b }, ce);
      const eyeF = st.eyes === 'kawaii' ? lin('k', -st.b, st.b, '#2c3866', '#0c0f1c') : rad('ir', '#a86c30', '#4a2a10');
      n.eyes = [-1, 1].map(side => this.buildEye(n.face, side, lidF, eyeF, `url(#${id('ce')})`));
      n.brows = [-1, 1].map(() => led
        ? el('rect', { x: -7, y: -1.8, width: 14, height: 3.6, rx: 1.8, fill: P.led }, n.face)
        : el('path', { d: 'M -7.5 1 Q 0 -2.5 7.5 1', fill: 'none', stroke: P.line, 'stroke-width': 3, 'stroke-linecap': 'round' }, n.face));
      n.mouth = el('g', null, n.face);
      const cm = el('clipPath', { id: id('cm') }, defs);
      n.mouthClip = el('path', null, cm);
      if (led) {
        n.mouthGlow = el('path', { fill: 'none', stroke: P.glow, 'stroke-width': 8, 'stroke-linejoin': 'round', 'stroke-linecap': 'round' }, n.mouth);
        n.mouthPath = el('path', { fill: 'none', stroke: P.led, 'stroke-width': 3, 'stroke-linejoin': 'round', 'stroke-linecap': 'round' }, n.mouth);
      } else {
        n.mouthPath = el('path', { fill: st.mouth, stroke: st.mouth, 'stroke-width': 2.6, 'stroke-linejoin': 'round', 'stroke-linecap': 'round' }, n.mouth);
        n.tongue = el('ellipse', { cx: 0, fill: '#ff7b8f', 'clip-path': `url(#${id('cm')})` }, n.mouth);
      }
      // Полоска на «животике» — ход передачи (как окошко для подписи на настоящих папках).
      n.label = el('g', { opacity: 0 }, n.flap);
      el('rect', { x: -21, y: -18, width: 42, height: 10, rx: 3.5, fill: led ? '#0b0f15' : 'rgba(255,255,255,.8)', stroke: led ? '#272e3a' : P.line, 'stroke-opacity': 0.35, 'stroke-width': 1.2 }, n.label);
      n.labelFill = el('rect', { x: -19, y: -16, width: 0, height: 6, rx: 2.2, fill: led ? P.led : '#1f7ae0' }, n.label);
      n.glint = el('rect', { x: -10, y: -130, width: 16, height: 170, fill: 'rgba(255,255,255,.55)' }, el('g', { 'clip-path': `url(#${id('cf')})` }, n.flap));
      if (led) {
        for (const [x, y] of [[-54, -74], [54, -74], [-54, -9], [54, -9]]) {
          el('circle', { cx: x, cy: y, r: 2.6, fill: '#8e99ab' }, n.flap);
          el('circle', { cx: x - 0.6, cy: y - 0.6, r: 0.9, fill: '#d5dbe5' }, n.flap);
        }
        n.status = el('circle', { cx: 42, cy: -9, r: 2.2, fill: '#3ddc84' }, n.flap);
      }
      if (this.variant === 'classic') {
        n.clip = el('path', { d: 'M 0 6 V -10 a 3.6 3.6 0 0 1 7.2 0 V 10 a 5.2 5.2 0 0 1 -10.4 0 V -5', fill: 'none', stroke: '#a3acb7', 'stroke-width': 1.9, 'stroke-linecap': 'round' }, n.flap);
      }

      n.slotFront = el('g', null, root);    // то, что Папыч держит перед собой
      n.arms = [-1, 1].map(() => {
        const g = el('g', null, root), a = {};
        if (st.line) a.out = el('path', { fill: 'none', stroke: P.line, 'stroke-width': st.limb + st.line * 2, 'stroke-linecap': 'round' }, g);
        a.path = el('path', { fill: 'none', stroke: P.limb, 'stroke-width': st.limb, 'stroke-linecap': 'round' }, g);
        if (led) a.joint = el('circle', { r: 3.6, fill: '#8e99ab', stroke: P.line, 'stroke-width': 1 }, g);
        a.hand = el('circle', { r: led ? 5.5 : 6.5, fill: P.hand, stroke: st.line ? P.line : P.limb, 'stroke-width': st.line || 1.6 }, g);
        return a;
      });
      // Указка со звёздочкой — для экскурсии.
      n.stick = el('g', { opacity: 0 }, root);
      n.stickLine = el('line', { stroke: '#d9b27a', 'stroke-width': 3.4, 'stroke-linecap': 'round' }, n.stick);
      n.stickTip = el('path', { d: STAR, fill: '#ffd43b', stroke: '#e09b00', 'stroke-width': 0.8 }, n.stick);
      // Ночной колпак — пауза.
      n.cap = el('g', { opacity: 0 }, root);
      el('path', { d: 'M -62 -117 C -60 -152 -18 -162 10 -139 C -2 -136 -13 -128 -14 -117 Z', fill: P.cap, stroke: 'rgba(0,0,0,.18)', 'stroke-width': 1 }, n.cap);
      for (const [x, y] of [[-46, -133], [-30, -144], [-34, -124], [-15, -138], [-52, -122]]) el('circle', { cx: x, cy: y, r: 1.6, fill: 'rgba(255,255,255,.8)' }, n.cap);
      el('rect', { x: -67, y: -122, width: 58, height: 10, rx: 5, fill: '#fff', stroke: 'rgba(0,0,0,.12)', 'stroke-width': 1 }, n.cap);
      el('circle', { cx: 11, cy: -139, r: 6.5, fill: '#fff', stroke: 'rgba(0,0,0,.12)', 'stroke-width': 1 }, n.cap);
      n.sweat = el('path', { d: 'M 0 -6 C 2.5 -2 5 1 5 4 A 5 5 0 0 1 -5 4 C -5 1 -2.5 -2 0 -6 Z', fill: '#8fd3ff', stroke: '#3b97d9', 'stroke-width': 1, opacity: 0 }, root);

      // Значок «нет связи» над головой и слой эффектов.
      n.off = el('g', { opacity: 0 }, svg);
      const gs = { fill: 'none', stroke: '#7a8494', 'stroke-width': 3, 'stroke-linecap': 'round' };
      el('circle', { cx: 0, cy: 0, r: 2.8, fill: '#7a8494' }, n.off);
      el('path', { d: 'M -7 -6 Q 0 -12 7 -6', ...gs }, n.off);
      el('path', { d: 'M -13 -11.5 Q 0 -22 13 -11.5', ...gs }, n.off);
      el('path', { d: 'M -19 -17 Q 0 -32 19 -17', ...gs }, n.off);
      el('path', { d: 'M -17 -27 L 15 4', fill: 'none', stroke: '#e5484d', 'stroke-width': 3.4, 'stroke-linecap': 'round' }, n.off);
      n.fx = el('g', null, svg);

      if (this.interactive) {
        root.addEventListener('pointerenter', () => this.hover(true));
        root.addEventListener('pointerleave', () => this.hover(false));
        root.addEventListener('click', e => { e.stopPropagation(); this.poke(); });
      }
      this.render(0);
    }

    buildEye(parent, side, lidF, eyeF, clip) {
      const st = this.st, P = this.pal, a = st.a, b = st.b, kind = st.eyes;
      const g = el('g', null, parent), e = { g, side };
      e.open = el('g', null, g);
      if (kind === 'led') {
        el('rect', { x: -a - 3.5, y: -b - 3.5, width: 2 * a + 7, height: 2 * b + 7, rx: a + 3.5, fill: P.glow }, e.open);
        el('rect', { x: -a, y: -b, width: 2 * a, height: 2 * b, rx: a, fill: P.led }, e.open);
        el('rect', { x: -a + 2.2, y: -b + 3, width: 2.8, height: 6, rx: 1.4, fill: 'rgba(255,255,255,.6)' }, e.open);
      } else {
        const kaw = kind === 'kawaii';
        el('ellipse', { rx: a, ry: b, fill: kaw ? eyeF : '#fff' }, e.open);
        const c = el('g', { 'clip-path': clip }, e.open);
        if (kaw) {
          el('ellipse', { cx: 0, cy: b * 0.62, rx: a * 0.62, ry: b * 0.26, fill: 'rgba(120,175,255,.5)' }, c);
        } else {
          el('ellipse', { cy: -b * 0.55, rx: a, ry: b * 0.5, fill: 'rgba(60,40,0,.07)' }, c);
          e.pupil = el('g', null, c);
          el('circle', { r: 8, fill: eyeF }, e.pupil);
          el('circle', { r: 4.4, fill: '#120a04' }, e.pupil);
        }
        e.hl = el('g', null, c);
        el('circle', { cx: kaw ? -3.4 : -2.6, cy: kaw ? -4.6 : -3.4, r: kaw ? 3.8 : 2.7, fill: '#fff' }, e.hl);
        el('circle', { cx: kaw ? 3.6 : 2.8, cy: kaw ? 3.4 : 2.6, r: kaw ? 1.8 : 1.25, fill: '#fff', opacity: 0.9 }, e.hl);
        if (!kaw) el('ellipse', { rx: a, ry: b, fill: 'none', stroke: P.line, 'stroke-width': 3.4 }, c);
        e.lidT = el('path', { fill: lidF }, c);
        e.lashT = el('path', { fill: 'none', stroke: P.line, 'stroke-width': 2.4, 'stroke-linecap': 'round' }, c);
        e.lidB = el('path', { fill: lidF }, c);
        e.lashB = el('path', { fill: 'none', stroke: P.line, 'stroke-width': 1.5, 'stroke-linecap': 'round', opacity: 0.55 }, c);
      }
      const color = kind === 'led' ? P.led : P.line, sa = kind === 'led' ? 9.5 : a + 1;
      const special = d => {
        const s = el('g', { opacity: 0 }, g);
        if (kind === 'led') el('path', { d, fill: 'none', stroke: P.glow, 'stroke-width': 8, 'stroke-linecap': 'round' }, s);
        el('path', { d, fill: 'none', stroke: color, 'stroke-width': 3.3, 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, s);
        return s;
      };
      e.happy = special(`M ${-sa} ${r2(b * 0.3)} Q 0 ${r2(-b * 0.95)} ${sa} ${r2(b * 0.3)}`);
      e.sleep = special(`M ${-sa} ${r2(-b * 0.05)} Q 0 ${r2(b * 0.62)} ${sa} ${r2(-b * 0.05)}`);
      e.spiral = special(spiralPath(Math.max(a, 9)));
      e.heart = el('g', { opacity: 0 }, g);
      el('path', { d: heartPath(Math.max(a, 9.5) * 1.1), fill: '#ff4d6d', stroke: kind === 'led' ? 'none' : '#c9184a', 'stroke-width': 1.2 }, e.heart);
      return e;
    }

    lids(e, u, lb, tilt) {
      const a = this.st.a, b = this.st.b, X = a * 1.4;
      const tIn = tilt * b * 0.6, tOut = -tilt * b * 0.2;
      const tl = e.side < 0 ? tOut : tIn, tr = e.side < 0 ? tIn : tOut;
      const y = lerp(-b * 1.3, b * 0.95, u), sag = b * 0.55;
      set(e.lidT, 'd', `M ${-X} ${-b * 1.8} L ${X} ${-b * 1.8} L ${X} ${r2(y + tr)} Q 0 ${r2(y + sag)} ${-X} ${r2(y + tl)} Z`);
      set(e.lashT, 'd', `M ${-X} ${r2(y + tl)} Q 0 ${r2(y + sag)} ${X} ${r2(y + tr)}`);
      const yb = lerp(b * 1.35, -b * 0.1, lb), bs = -b * 0.5;
      set(e.lidB, 'd', `M ${-X} ${b * 1.8} L ${X} ${b * 1.8} L ${X} ${r2(yb)} Q 0 ${r2(yb + bs)} ${-X} ${r2(yb)} Z`);
      set(e.lashB, 'd', `M ${-X} ${r2(yb)} Q 0 ${r2(yb + bs)} ${X} ${r2(yb)}`);
    }

    render(dt) {
      const s = k => this.sp[k].x, n = this.n, st = this.st, led = st.eyes === 'led', t = this.t;
      const open = clamp(s('open'), 0, 1.15);
      this.breath += dt * TAU * (this.sleeping ? 0.2 : 0.32);
      const br = Math.sin(this.breath) * (this.sleeping ? 0.026 : 0.013);
      const air = clamp(-this.jy / 70);
      const sq = s('squash') - br - clamp(-this.jv / 700) * 0.08;
      const sy = 1 - sq, sx = (1 + sq * 0.6) * Math.cos(s('spin'));
      const fx = s('fx'), fy = s('fy');
      const shake = this.shake ? Math.sin(t * 70) * this.shake : 0;
      set(n.root, 'transform', `translate(${r2(shake)} ${r2(this.jy)}) rotate(${r2(s('lean') + fx * 2.5)}) scale(${r2(sx)} ${r2(sy)})`);
      set(n.shadow, 'rx', r2(60 * Math.abs(sx) * (1 - air * 0.45)));
      set(n.shadow, 'opacity', r2(1 - air * 0.6));
      const dangle = clamp(-this.jy / 18) * 5;
      n.feet.forEach((f, i) => { set(f, 'cx', r2((i ? 1 : -1) * (26 + Math.max(0, sq) * 30))); set(f, 'cy', r2(-1.5 + dangle)); });
      set(n.back, 'transform', `translate(${r2(-fx * 1.6)} ${r2(-fy)})`);
      if (n.curl) set(n.curl, 'transform', `rotate(${r2(Math.sin(t * 2.2) * 4 - this.sp.squash.v * 2)} -40 -116)`);
      if (n.ant) {
        set(n.ant, 'opacity', r2(1 - smooth(0.2, 0.6, s('cap'))));
        set(n.antBall, 'opacity', r2(0.55 + 0.45 * Math.sin(t * 4)));
        set(n.status, 'opacity', r2(this.busy ? 0.4 + 0.6 * (Math.sin(t * 14) > 0) : 0.9));
      }
      set(n.papers, 'transform', `translate(${r2(-fx)} ${r2(-fy * 0.6 + s('paper'))})`);
      n.sheets.forEach((p, i) => set(p.g, 'transform',
        `translate(${p.x} ${r2(p.y + Math.sin(t * 1.7 + p.ph) * 0.7 - open * 3 * (i - 1))}) rotate(${r2(p.r + Math.sin(t * 1.3 + p.ph) * 1.2)})`));

      const fd = flapPath(st.r, open);
      set(n.flapPath, 'd', fd); set(n.flapClip, 'd', fd);
      if (n.seam) set(n.seam, 'd', flapPath(st.r, open, 5));
      const yt = -HF + open * 30;
      set(n.rim, 'd', `M ${r2(-W / 2 + 10 - open * 6)} ${r2(yt + 1.3)} L ${r2(W / 2 - 10 + open * 6)} ${r2(yt + 1.3)}`);
      if (n.clip) set(n.clip, 'transform', `translate(36 ${r2(yt)})`);
      const k = 1 - 0.16 * open, ty = open * 22 + st.ey * (1 - k);
      set(n.face, 'transform', `translate(${r2(fx * 4)} ${r2(fy * 3 + ty)}) scale(1 ${r2(k)})`);
      if (n.screen) set(n.screen, 'transform', `translate(0 ${r2(ty)}) scale(1 ${r2(k)})`);

      // глаза
      const u = clamp(Math.max(s('lidTop'), this.blink)), lb = clamp(s('lidBot')), tilt = s('lidTilt'), es = s('eyeS');
      const special = clamp(Math.max(s('happyEyes'), s('sleepEyes'), s('spiralEyes'), s('heartEyes')));
      n.eyes.forEach(e => {
        const L = e.side < 0, dx = s(L ? 'elx' : 'erx'), dy = s(L ? 'ely' : 'ery');
        set(e.g, 'transform', `translate(${e.side * st.ex} ${st.ey}) scale(${r2(es)})`);
        set(e.open, 'opacity', r2(1 - special));
        if (led) {
          set(e.open, 'transform', `translate(${r2(dx * 5.5)} ${r2(dy * 5)}) scale(1 ${r2(Math.max(0.08, 1 - u * 0.95 - lb * 0.3))})`);
        } else if (st.eyes === 'kawaii') {
          set(e.open, 'transform', `translate(${r2(dx * 3.6)} ${r2(dy * 3)})`);
          set(e.hl, 'transform', `translate(${r2(-dx * 1.2)} ${r2(-dy)})`);
          this.lids(e, u, lb, tilt);
        } else {
          set(e.pupil, 'transform', `translate(${r2(dx * 5.4)} ${r2(dy * 6)})`);
          set(e.hl, 'transform', `translate(${r2(dx * 3.6)} ${r2(dy * 4.2)})`);
          this.lids(e, u, lb, tilt);
        }
        set(e.happy, 'opacity', r2(clamp(s('happyEyes'))));
        set(e.sleep, 'opacity', r2(clamp(s('sleepEyes'))));
        set(e.spiral, 'opacity', r2(clamp(s('spiralEyes'))));
        set(e.spiral, 'transform', `rotate(${r2((t * 420 * e.side) % 360)})`);
        set(e.heart, 'opacity', r2(clamp(s('heartEyes'))));
        set(e.heart, 'transform', `scale(${r2(1 + Math.sin(t * 9) * 0.1)})`);
      });
      const brA = clamp(s('brA')), brY = s('brY'), brT = s('brT'), asym = s('brAsym');
      n.brows.forEach((b, i) => {
        const side = i ? 1 : -1, y = st.ey - st.b * es - 7 - brY - (side < 0 ? asym * 5 : -asym * 1.5);
        set(b, 'transform', `translate(${side * st.ex} ${r2(y)}) rotate(${r2(-side * brT * 15 + (side < 0 ? -asym * 8 : 0))})`);
        set(b, 'opacity', r2(brA));
      });
      n.cheeks.forEach(c => set(c, 'opacity', r2(clamp(s('blush') * (led ? 0.45 : 0.6)))));

      // рот
      const mo = Math.max(0, s('mO')), mw = Math.max(2, s('mW')), mc = s('mC'), wob = s('wob') * (0.85 + 0.15 * Math.sin(t * 6));
      const md = mouthPath(mw, mc, mo, wob);
      set(n.mouthPath, 'd', md); set(n.mouthClip, 'd', md);
      if (n.mouthGlow) set(n.mouthGlow, 'd', md);
      set(n.mouth, 'transform', `translate(0 ${st.my})`);
      set(n.mouth, 'opacity', r2(1 - smooth(0.25, 0.6, open)));
      if (n.tongue) {
        set(n.tongue, 'cy', r2((mc + mo) * 0.8));
        set(n.tongue, 'rx', r2(mw * 0.55));
        set(n.tongue, 'ry', r2(mo * 0.38 + 0.1));
        set(n.tongue, 'opacity', r2(smooth(2, 5, mo)));
      }
      set(n.label, 'opacity', r2(clamp(s('progA'))));
      set(n.label, 'transform', `translate(0 ${r2(open * 4)})`);
      set(n.labelFill, 'width', r2(38 * clamp(s('prog'))));
      set(n.glint, 'transform', `translate(${r2(this.glintX)} 0) rotate(22)`);

      // руки: плечо → кисть, локоть чуть наружу
      const bob = br * 30;
      n.arms.forEach((a, i) => {
        const side = i ? 1 : -1, x0 = side * (W / 2 - 4), y0 = -44;
        const hx = s(i ? 'hrx' : 'hlx'), hy = s(i ? 'hry' : 'hly') + bob;
        const reach = Math.max(0, y0 - hy);   // поднятые руки — локти шире
        const mx = (x0 + hx) / 2 + side * (9 + reach * 0.35), my = (y0 + hy) / 2 + 7 + reach * 0.1;
        const d = `M ${x0} ${y0} Q ${r2(mx)} ${r2(my)} ${r2(hx)} ${r2(hy)}`;
        set(a.path, 'd', d);
        if (a.out) set(a.out, 'd', d);
        if (a.joint) { set(a.joint, 'cx', r2((x0 + 2 * mx + hx) / 4)); set(a.joint, 'cy', r2((y0 + 2 * my + hy) / 4)); }
        set(a.hand, 'cx', r2(hx)); set(a.hand, 'cy', r2(hy));
      });
      const stickA = clamp(s('stickA'));
      set(n.stick, 'opacity', r2(stickA));
      if (stickA > 0.01) {
        const R = this.stickSide > 0, hx = s(R ? 'hrx' : 'hlx'), hy = s(R ? 'hry' : 'hly') + bob;
        const tx = hx + Math.cos(this.stickAng) * 66, ty = hy + Math.sin(this.stickAng) * 66;
        set(n.stickLine, 'x1', r2(hx)); set(n.stickLine, 'y1', r2(hy)); set(n.stickLine, 'x2', r2(tx)); set(n.stickLine, 'y2', r2(ty));
        set(n.stickTip, 'transform', `translate(${r2(tx)} ${r2(ty)}) rotate(${r2((t * 80) % 360)}) scale(${r2(0.9 + Math.sin(t * 6) * 0.15)})`);
      }
      const cap = s('cap');
      set(n.cap, 'opacity', r2(clamp(cap * 3)));
      set(n.cap, 'transform', `translate(-38 -114) scale(${r2(Math.max(0.001, cap))}) rotate(${r2(Math.sin(t * 1.3) * 3)}) translate(38 114)`);
      const ph = (t * 0.7) % 1;
      set(n.sweat, 'opacity', r2(clamp(s('sweat')) * (1 - ph)));
      set(n.sweat, 'transform', `translate(55 ${r2(-96 + ph * 20)})`);
      set(n.off, 'opacity', r2(clamp(s('offA'))));
      set(n.off, 'transform', `translate(0 ${r2(-140 + Math.sin(t * 2.2) * 3 + this.jy)})`);
      const sat = r2(clamp(s('sat'), 0.2, 1));
      if (Math.abs(sat - this.lastSat) > 0.01) { this.lastSat = sat; this.svg.style.filter = sat > 0.98 ? '' : `saturate(${sat})`; }
    }

    // ---------- жизнь ----------
    tick(dt) {
      this.t += dt;
      const t = this.t;
      for (let i = this.timers.length - 1; i >= 0; i--) {
        const w = this.timers[i];
        if (w.tok !== this.tok) this.timers.splice(i, 1);
        else if (t >= w.at) { this.timers.splice(i, 1); w.res(); }
      }
      for (let i = this.tweens.length - 1; i >= 0; i--) {
        const w = this.tweens[i];
        if (w.tok !== this.tok) { this.tweens.splice(i, 1); continue; }
        w.age += dt;
        const k = Math.min(1, w.age / w.dur);
        w.fn(k);
        if (k >= 1) { this.tweens.splice(i, 1); w.res(); }
      }
      this.idle();
      this.gaze(dt);
      this.blinking(dt);
      if (this.pointAt) this.aimStick();
      if (this.waveT > 0) {
        this.waveT -= dt;
        const w = Math.sin(t * 15), c = Math.cos(t * 15);
        this.sp.hrx.to = 80 + w * 8; this.sp.hry.to = -92 + c * 4;
        if (this.wave2) { this.sp.hlx.to = -80 + w * 8; this.sp.hly.to = -92 - c * 4; }
        if (this.waveT <= 0) this.rest();
      }
      const steps = Math.max(1, Math.ceil(dt * 120)), h = dt / steps;
      for (let j = 0; j < steps; j++) {
        for (const k in this.sp) { const p = this.sp[k]; p.v += (p.k * (p.to - p.x) - p.d * p.v) * h; p.x += p.v * h; }
      }
      if (this.jy < 0 || this.jv < 0) {
        this.jv += 1700 * dt; this.jy += this.jv * dt;
        if (this.jy >= 0) { const hit = this.jv; this.jy = 0; this.jv = 0; this.sp.squash.v += hit * 0.0022; if (hit > 380) this.dust(); }
      }
      if (this.sleeping) { this.zIn -= dt; if (this.zIn <= 0) { this.zIn = 1.5; this.zzz(); } }
      this.render(dt);
      for (let i = this.parts.length - 1; i >= 0; i--) {
        const p = this.parts[i];
        p.age += dt;
        const k = p.age / p.life;
        if (k >= 1) { p.node.remove(); this.parts.splice(i, 1); } else p.fn(k, dt);
      }
    }

    // Куда смотреть: сценка → чужой взгляд → курсор → сам оглядывается.
    gaze(dt) {
      const r = this.svg.getBoundingClientRect();
      if (!r.width) return;
      this.rect = r;
      const vb = this.vb, sc = r.width / vb[2], st = this.st;
      const toClient = (x, y) => [r.left + (x - vb[0]) * sc, r.top + (y - vb[1]) * sc];
      let target = null, front = false;
      if (this.look) { const p = this.look(); target = toClient(p[0], p[1]); }
      else if (this.glance && this.t < this.glance.until) target = this.glance.p;
      else if (input.x != null && now() - input.t < 4) target = [input.x, input.y];
      else {
        this.wanderIn -= dt;
        if (this.wanderIn <= 0 || !this.wander) {
          this.wanderIn = rnd(1.2, 3.4);
          this.wander = Math.random() < 0.3 ? 'front' : [rnd(-300, 300), rnd(-260, 80)];
        }
        if (this.wander === 'front') front = true; else target = toClient(this.wander[0], this.wander[1]);
      }
      const sp = this.sp;
      if (front || this.sleeping) { for (const k of ['elx', 'ely', 'erx', 'ery', 'fx', 'fy']) sp[k].to = 0; return; }
      const K = Math.max(70, r.width * 0.55);
      const proj = (x, y) => {
        const c = toClient(x, y), dx = target[0] - c[0], dy = target[1] - c[1], d = Math.hypot(dx, dy);
        if (d < 0.001) return [0, 0];
        const f = d / Math.sqrt(d * d + K * K);
        return [(dx / d) * f, (dy / d) * f];
      };
      const ey = st.ey + this.jy, L = proj(-st.ex, ey), R = proj(st.ex, ey), F = proj(0, ey);
      sp.elx.to = L[0]; sp.ely.to = L[1]; sp.erx.to = R[0]; sp.ery.to = R[1]; sp.fx.to = F[0]; sp.fy.to = F[1];
    }

    blinking(dt) {
      if (this.blinkAge >= 0) {
        const a = (this.blinkAge += dt);
        this.blink = a < 0.07 ? a / 0.07 : a < 0.12 ? 1 : a < 0.24 ? 1 - (a - 0.12) / 0.12 : 0;
        if (a >= 0.24) this.blinkAge = -1;
        return;
      }
      this.blink = 0;
      if ((this.blinkIn -= dt) > 0) return;
      this.blinkIn = Math.random() < 0.18 ? 0.35 : rnd(2, 5.5);
      if (!this.sleeping && this.sp.happyEyes.to < 0.5 && this.sp.spiralEyes.to < 0.5) this.blinkAge = 0;
    }

    idle() {
      if (this.busy || this.state.paused || this.autoSleep) return;
      const quiet = now() - input.t;
      if (quiet > 55) { this.autoSleep = true; this.sleeping = true; this.expr('sleep'); }
      else if (quiet > 28 && !this.yawned) { this.yawned = true; this.yawn(); }
    }

    wake() {
      this.yawned = false;
      if (this.autoSleep) { this.autoSleep = false; this.sleeping = false; if (!this.state.paused) this.startle(); }
    }

    expr(name) {
      this.mood = name;
      const e = EXPR[name] || {};
      for (const k in BASE) this.sp[k].to = e[k] ?? BASE[k];
    }
    baseMood() {
      if (this.state.paused || this.autoSleep) return 'sleep';
      if (this.state.offline) return 'sad';
      return this.hovered ? 'happy' : 'neutral';
    }
    rest() {
      const sp = this.sp;
      sp.hlx.to = -68; sp.hly.to = -12; sp.hrx.to = 68; sp.hry.to = -12;
    }
    jump(p = 1) { if (this.jy === 0) this.jv = -220 - 300 * p; }

    // Сценки: новая отменяет старую (её ожидания просто не продолжатся).
    begin(scene) {
      this.tok++; this.busy = true; this.scene = scene;
      this.sleeping = false; this.waveT = 0; this.wave2 = false; this.shake = 0; this.look = null; this.glance = null; this.glintX = -200;
      this.n.slotIn.replaceChildren(); this.n.slotFront.replaceChildren();
      const sp = this.sp;
      sp.open.to = 0; sp.progA.to = 0; sp.sweat.to = 0; sp.lean.to = 0; sp.squash.to = 0;
      sp.spin.x = sp.spin.to = 0;
      this.rest();
    }
    settle() {
      if (this.scene?.startsWith('chat-')) this.n.slotFront.replaceChildren();
      this.tok++; this.busy = false; this.scene = null; this.look = null; this.shake = 0; this.waveT = 0; this.wave2 = false;
      const sp = this.sp, st = this.state;
      sp.open.to = 0; sp.squash.to = 0; sp.lean.to = 0; sp.progA.to = 0; sp.sweat.to = 0;
      sp.cap.to = st.paused ? 1 : 0;
      if (this.progress != null) { sp.progA.to = 1; sp.prog.to = this.progress; }
      sp.sat.to = st.offline && !st.paused ? 0.35 : 1;
      sp.offA.to = st.offline ? 1 : 0;
      this.rest();
      this.sleeping = st.paused || this.autoSleep;
      this.expr(this.baseMood());
      if (st.offline && !st.paused) this.look = () => [Math.sin(this.t * 0.8) * 260, -40];   // ищет связь
    }
    wait(sec) { const tok = this.tok; return new Promise(res => this.timers.push({ at: this.t + sec, tok, res })); }
    tween(dur, fn) { const tok = this.tok; return new Promise(res => this.tweens.push({ age: 0, dur, fn, tok, res })); }

    toLocal(cx, cy) {
      const r = this.rect || this.svg.getBoundingClientRect(), sc = r.width / this.vb[2];
      return [this.vb[0] + (cx - r.left) / sc, this.vb[1] + (cy - r.top) / sc];
    }
    // Точка рта на экране — куда бросать файл.
    mouthPoint() {
      const r = this.rect || this.svg.getBoundingClientRect(), sc = r.width / this.vb[2];
      return [r.left + (0 - this.vb[0]) * sc, r.top + (-90 - this.vb[1]) * sc];
    }

    hover(on) {
      this.hovered = on;
      if (this.busy || this.sleeping) return;
      if (on) {
        this.expr('happy');
        if (this.t - this.lastWave > 5) { this.lastWave = this.t; this.waveT = 1.3; }
      } else this.expr(this.baseMood());
    }
    poke() {
      const t = this.t;
      this.clicks = this.clicks.filter(c => t - c < 1.4);
      this.clicks.push(t);
      if (this.busy && this.scene !== 'boop' && this.scene !== 'sneeze') return;
      if (this.clicks.length >= 5) { this.clicks = []; this.dizzy(); }
      else if (this.clicks.length === 3) this.sneeze();
      else if (!this.busy) this.boop();
    }

    // Файл тащат над экраном: чем ближе к Папычу, тем шире рот.
    dragOver(x, y) {
      if (this.busy && this.scene !== 'drag') return;
      if (!this.busy) { this.tok++; this.busy = true; this.scene = 'drag'; this.sleeping = false; this.autoSleep = false; }
      const [lx, ly] = this.toLocal(x, y), d = Math.hypot(lx, ly + 90), near = clamp(1 - (d - 30) / 220);
      this.sp.open.to = near * 1.05;
      this.sp.squash.to = -near * 0.05;
      if (this.mood !== (near > 0.12 ? 'excited' : 'surprised')) this.expr(near > 0.12 ? 'excited' : 'surprised');
    }
    dragEnd() { if (this.scene === 'drag') this.settle(); }
    glanceAt(x, y, sec = 1.5) {
      this.glance = { p: [x, y], until: this.t + sec };
      if (this.busy) return;
      this.expr('surprised');
      setTimeout(() => { if (!this.busy) this.expr(this.baseMood()); }, sec * 1000);
    }

    // ---------- сценки ----------
    async swallow(file = {}, opt = {}) {
      this.begin('swallow');
      const it = this.makeItem(file), from = opt.from ? this.toLocal(opt.from[0], opt.from[1]) : [95, -150];
      const apex = [0, -132], pos = [...from];
      const place = (x, y, s, r) => set(it, 'transform', `translate(${r2(x)} ${r2(y)}) rotate(${r2(r)}) scale(${r2(s)})`);
      this.n.slotFront.appendChild(it);
      place(pos[0], pos[1], 1, -25);
      this.look = () => pos;
      this.expr('excited');
      this.sp.open.to = 1;
      await this.tween(0.42, k => {
        const e = ease.out(k);
        pos[0] = lerp(from[0], apex[0], e); pos[1] = lerp(from[1], apex[1], e) - Math.sin(k * Math.PI) * 30;
        place(pos[0], pos[1], lerp(1, 0.78, e), lerp(-25, 8, e));
      });
      this.n.slotIn.appendChild(it);
      await this.tween(0.24, k => { const e = ease.in(k); pos[1] = lerp(apex[1], -40, e); place(0, pos[1], lerp(0.78, 0.66, e), 8 - 8 * e); });
      it.remove();
      this.sp.open.to = 0; this.sp.squash.v += 5.5; this.sp.paper.v += 70;
      this.look = null; this.expr('happy');
      await this.wait(0.16);
      for (let i = 0; i < 2; i++) {          // жуёт
        this.sp.open.to = 0.22; await this.wait(0.11);
        this.sp.open.to = 0; this.sp.squash.v += 1.5; await this.wait(0.14);
      }
      this.expr('joy'); this.sparkles(2);
      await this.wait(0.55);
      if (opt.launch) await this.launch(opt.launch);
      else if (opt.send !== false) await this.sendOut(opt.color);
      this.settle();
    }

    async sendOut(color = '#1f7ae0') {
      this.expr('focused');
      this.look = () => [0, -8];             // смотрит на полоску на животике
      this.sp.prog.x = 0; this.sp.prog.to = 0; this.sp.progA.to = 1;
      await this.tween(2.3, k => { this.sp.prog.to = ease.inOut(k); this.sp.squash.to = Math.max(0, Math.sin(k * 28)) * 0.035; });
      this.sp.squash.to = 0; this.sp.progA.to = 0;
      await this.launch(color);
    }

    // Выпускает бумажный самолётик и радуется: «доставлено».
    async launch(color = '#1f7ae0') {
      this.expr('excited');
      const plane = { pos: [0, -96] };
      this.look = () => plane.pos;
      this.sp.open.to = 0.6;
      await this.wait(0.14);
      this.plane(color, plane); this.sp.squash.v -= 4;
      await this.wait(0.22);
      this.sp.open.to = 0;
      await this.wait(0.8);
      this.look = null; this.expr('joy'); this.badge(); this.jump(0.7); this.sparkles(3);
      await this.wait(1.2);
    }

    async receive(file = {}) {
      this.begin('receive');
      this.bubble('!', 1.0, 44); this.expr('surprised');
      await this.wait(0.3);
      this.jump(0.6); this.expr('excited'); this.sp.open.to = 1;
      await this.wait(0.2);
      const it = this.makeItem(file), pos = [0, -40];
      const place = (x, y, s, r = 0, o = 1) => { set(it, 'transform', `translate(${r2(x)} ${r2(y)}) rotate(${r2(r)}) scale(${r2(s)})`); set(it, 'opacity', r2(o)); };
      this.n.slotIn.appendChild(it);
      place(0, -40, 0.7);
      this.look = () => pos;
      await this.tween(0.42, k => { pos[1] = lerp(-40, -128, ease.back(k)); place(0, pos[1], lerp(0.7, 1, k)); });
      this.n.slotFront.appendChild(it);
      const sp = this.sp;
      sp.open.to = 0; sp.hlx.to = -17; sp.hly.to = -114; sp.hrx.to = 17; sp.hry.to = -114;
      this.expr('joy'); this.sparkles(3);
      await this.tween(1.5, k => {
        const w = Math.sin(k * Math.PI * 4) * 5;
        pos[0] = w; place(w, -130 + Math.abs(w) * 0.3, 1, w * 1.2);
        sp.hlx.to = -17 + w; sp.hrx.to = 17 + w;
      });
      this.rest(); this.expr('happy'); this.look = null;
      await this.tween(0.4, k => { const e = ease.in(k); place(0, lerp(-128, -60, e), lerp(1, 1.9, e), 0, 1 - e); });   // «держи!»
      it.remove();
      await this.wait(0.3);
      this.settle();
    }

    async heavy(label = '12 ГБ') {
      this.begin('heavy');
      const box = this.makeBox(label), pos = [0, -230], sp = this.sp;
      const place = (x, y, r = 0) => set(box, 'transform', `translate(${r2(x)} ${r2(y)}) rotate(${r2(r)})`);
      this.n.slotFront.appendChild(box);
      place(0, -230);
      this.look = () => pos; this.expr('surprised');
      sp.hlx.to = -40; sp.hly.to = -60; sp.hrx.to = 40; sp.hry.to = -60;
      await this.tween(0.45, k => { pos[1] = lerp(-230, -16, ease.in(k)); place(0, pos[1], (1 - k) * 12); });
      sp.squash.v += 9; this.dust(); this.expr('strain'); sp.sweat.to = 1; this.shake = 0.8;
      sp.hlx.to = -33; sp.hly.to = -18; sp.hrx.to = 33; sp.hry.to = -18;
      this.look = null;
      await this.tween(2.6, k => {
        sp.squash.to = 0.09 + Math.sin(k * 40) * 0.01;
        place(Math.sin(this.t * 33) * 0.8, -16 + Math.sin(this.t * 30) * 0.6);
        if (Math.random() < 0.035) this.puff(rnd(-30, 30), -112, 0.6, rnd(-10, 10), -30);
      });
      this.shake = 0; sp.squash.to = 0;
      this.expr('excited'); sp.open.to = 1; sp.squash.v -= 6;   // подкинул — и в рот
      await this.tween(0.35, k => { const e = ease.out(k); pos[1] = lerp(-16, -150, e); place(0, pos[1], e * -10); });
      this.n.slotIn.appendChild(box);
      this.look = () => pos;
      await this.tween(0.25, k => { pos[1] = lerp(-150, -30, ease.in(k)); place(0, pos[1]); });
      box.remove();
      sp.open.to = 0; sp.squash.v += 9; sp.paper.v += 120; this.dust(); this.look = null;
      this.rest(); this.expr('dizzy');
      await this.wait(0.7);
      this.expr('happy'); sp.hrx.to = 44; sp.hry.to = -86;   // вытирает пот
      await this.tween(0.6, k => { sp.hrx.to = 44 - Math.sin(k * Math.PI) * 26; });
      sp.sweat.to = 0; this.rest(); this.expr('joy'); this.sparkles(2);
      await this.wait(0.8);
      this.settle();
    }

    async error() {
      this.begin('error');
      this.bubble('?', 2.2, 46); this.expr('confused'); this.sp.lean.to = -8;
      await this.tween(1.6, k => { this.sp.hrx.to = 34 + Math.sin(k * 40) * 4; this.sp.hry.to = -104 + Math.cos(k * 40) * 2; });   // чешет макушку
      this.sp.lean.to = 0; this.rest(); this.expr('worried');
      await this.wait(0.6);
      this.settle();
    }

    async hello() {
      this.begin('hello');
      this.expr('love'); this.hearts(4); this.waveT = 1.9; this.wave2 = true;
      this.jump(0.5);
      await this.wait(0.6);
      this.jump(0.5);
      await this.wait(1.4);
      this.settle();
    }

    async update() {
      this.begin('update');
      this.expr('excited'); this.sparkles(4);
      await this.wait(0.25);
      this.jump(1);
      await this.tween(0.75, k => { this.sp.spin.x = this.sp.spin.to = ease.inOut(k) * TAU; });
      this.sp.spin.x = this.sp.spin.to = 0;
      this.expr('joy'); this.sparkles(4);
      await this.tween(0.7, k => { this.glintX = lerp(-110, 120, ease.inOut(k)); });
      this.glintX = -200;
      await this.wait(0.6);
      this.settle();
    }

    async yawn() {
      this.begin('yawn');
      const sp = this.sp;
      this.expr('yawn'); sp.squash.to = -0.07;
      sp.hlx.to = -34; sp.hly.to = -118; sp.hrx.to = 34; sp.hry.to = -118;
      await this.wait(1.4);
      this.rest(); sp.squash.to = 0; this.expr('sleepy');
      await this.wait(0.7);
      this.settle();
    }

    async delivered(color) {
      this.begin('delivered');
      await this.launch(color);
      this.settle();
    }

    // Файл получен и сохранён.
    async done() {
      this.begin('done');
      this.expr('joy'); this.badge(); this.jump(0.6); this.sparkles(3);
      await this.wait(1.4);
      this.settle();
    }

    wave(sec = 1.3) {
      if (this.busy || this.sleeping) return;
      this.lastWave = this.t; this.waveT = sec; this.expr('happy');
      setTimeout(() => { if (!this.busy) this.expr(this.baseMood()); }, sec * 1000 + 300);
    }

    async boop() {
      this.begin('boop');
      this.sp.squash.v += 7; this.expr('joy'); this.jump(0.3);
      await this.wait(0.7);
      this.settle();
    }

    async sneeze() {
      this.begin('sneeze');
      const sp = this.sp;
      this.expr('squint');
      for (const d of [0.25, 0.45]) {          // а… а…
        sp.lean.to = -5 - d * 8; sp.open.to = d * 0.5; sp.squash.to = -0.04 - d * 0.05;
        await this.wait(0.42);
      }
      sp.lean.to = 9; sp.squash.to = 0; sp.squash.v += 10; sp.open.to = 1; this.expr('sneeze');   // апчхи!
      for (let i = 0; i < 4; i++) this.flyPaper(i);
      this.puff(-18, -98, 1.2, -30, -24); this.puff(16, -100, 1.4, 30, -26); this.puff(0, -104, 1.1, 0, -36);
      await this.wait(0.3);
      sp.open.to = 0; sp.lean.to = 0; this.expr('shy');
      await this.wait(1.1);
      this.settle();
    }

    async dizzy() {
      this.begin('dizzy');
      this.expr('dizzy'); this.stars(2.6);
      await this.tween(2.6, k => { this.sp.lean.to = Math.sin(k * Math.PI * 7) * 9 * (1 - k * 0.6); });
      this.sp.lean.to = 0; this.expr('worried');
      await this.wait(0.4);
      this.settle();
    }

    // Чат: микрофон/наушники, рот и руки двигаются по реальному уровню звука.
    // Раскрывает мини-чат руками; облачный конверт показывает на животике.
    async chatUnfold() {
      this.begin('chat-unfold'); this.expr('happy');
      this.sp.hlx.to = -55; this.sp.hly.to = 18;
      this.sp.hrx.to = 55; this.sp.hry.to = 18;
      this.sp.squash.v += 4;
      await this.wait(0.42);
      if (this.scene === 'chat-unfold') this.settle();
    }
    async chatCloud() {
      this.begin('chat-cloud'); this.expr('happy');
      el('path', {d:'M -20 -25 H 20 V -4 H -20 Z M -20 -25 L 0 -11 L 20 -25',fill:'none',stroke:'#60cdff','stroke-width':3}, this.n.slotFront);
      this.sp.hrx.to = 42; this.sp.hry.to = -35;
      await this.wait(1.1);
      if (this.scene === 'chat-cloud') this.settle();
    }
    chatVoice(mode, level = 0) {
      if (this.scene !== 'chat-' + mode) {
        this.begin('chat-' + mode);
        this.expr(mode === 'record' ? 'focused' : mode === 'pause' ? 'neutral' : 'happy');
        el('path', { d: 'M -51 -90 Q -50 -146 0 -146 Q 50 -146 51 -90', fill: 'none', stroke: '#60cdff', 'stroke-width': 7 }, this.n.slotFront);
        el('rect', { x: -58, y: -105, width: 12, height: 28, rx: 5, fill: '#60cdff' }, this.n.slotFront);
        el('rect', { x: 46, y: -105, width: 12, height: 28, rx: 5, fill: '#60cdff' }, this.n.slotFront);
        if (mode === 'record') {
          el('rect', { x: 35, y: -59, width: 14, height: 24, rx: 7, fill: '#f66c7d' }, this.n.slotFront);
          el('path', { d: 'M 42 -35 V -20 M 32 -19 H 52', stroke: '#f66c7d', 'stroke-width': 4 }, this.n.slotFront);
        }
      }
      const v = clamp(level);
      this.sp.mO.to = mode === 'play' ? 2 + v * 18 : 2;
      this.sp.hrx.to = mode === 'record' ? 42 : 60 + v * 10;
      this.sp.hry.to = mode === 'record' ? -37 : -35 - v * 28;
      this.sp.hlx.to = -55 - v * 8;
      this.sp.hly.to = -30 - v * 18;
    }

    // Полоска на животике: общий ход загрузок (null — убрать).
    setProgress(v) {
      this.progress = v == null ? null : clamp(v);
      if (this.busy) return;
      this.sp.progA.to = this.progress == null ? 0 : 1;
      if (this.progress != null) this.sp.prog.to = this.progress;
    }

    // Выплёвывает что-то (файл летит дальше уже в окне программы).
    async spit() {
      this.begin('spit');
      const sp = this.sp;
      this.expr('excited'); sp.open.to = 0.9; sp.squash.v -= 5; sp.lean.to = 6;
      await this.wait(0.2);
      sp.open.to = 0; sp.lean.to = 0; this.expr('happy');
      await this.wait(0.6);
      this.settle();
    }

    // Отказались от файла — пожимает плечами.
    async shrug() {
      this.begin('shrug');
      const sp = this.sp;
      this.expr('worried'); sp.hlx.to = -76; sp.hly.to = -48; sp.hrx.to = 76; sp.hry.to = -48; sp.squash.to = 0.05;
      await this.wait(0.55);
      this.rest(); sp.squash.to = 0; this.expr('neutral');
      await this.wait(0.3);
      this.settle();
    }

    // Летит куда-то (окно двигает его само): поджимает руки и смотрит вперёд.
    async leap(on) {
      if (on) {
        this.begin('leap');
        const sp = this.sp;
        this.expr('excited'); sp.hlx.to = -40; sp.hly.to = -70; sp.hrx.to = 40; sp.hry.to = -70; sp.squash.to = -0.08;
      } else {
        this.sp.squash.to = 0; this.sp.squash.v += 9; this.dust(); this.rest(); this.expr('joy');
        await this.wait(0.6);
        this.settle();
      }
    }

    async startle() {
      this.begin('startle');
      this.expr('surprised'); this.jump(0.45);
      await this.wait(0.7);
      this.settle();
    }

    async pause(on = !this.state.paused) {
      this.state.paused = on;
      this.begin('pause');
      const sp = this.sp;
      if (on) {
        sp.cap.to = 1; this.expr('sleepy');
        await this.wait(0.8);
      } else {
        sp.cap.to = 0; this.expr('surprised'); this.jump(0.4);
        await this.wait(0.5);
        this.expr('yawn'); sp.hlx.to = -34; sp.hly.to = -118; sp.hrx.to = 34; sp.hry.to = -118;
        await this.wait(1.1);
      }
      this.settle();
    }

    async offline(on = !this.state.offline) {
      this.state.offline = on;
      this.begin('offline');
      if (on) {
        this.sp.offA.to = 1; this.expr('worried');
        await this.wait(0.6);
      } else {
        this.sp.offA.to = 0; this.sp.sat.to = 1; this.expr('joy'); this.jump(0.5); this.sparkles(2);
        await this.wait(0.9);
      }
      this.settle();
    }

    // ---------- предметы и эффекты ----------
    // Указка: показать на точку экрана (null — убрать).
    point(x, y) {
      if (x == null) {
        this.pointAt = null;
        this.sp.stickA.to = 0;
        if (!this.busy) this.rest();
        return;
      }
      this.pointAt = [x, y];
      this.sp.stickA.to = 1;
    }
    aimStick() {
      const [lx, ly] = this.toLocal(this.pointAt[0], this.pointAt[1]);
      this.stickSide = lx >= 0 ? 1 : -1;
      const sx = this.stickSide * (W / 2 - 4), sy = -44, a = Math.atan2(ly - sy, lx - sx);
      this.stickAng = a;
      const hand = this.stickSide > 0 ? ['hrx', 'hry'] : ['hlx', 'hly'];
      this.sp[hand[0]].to = sx + Math.cos(a) * 34;
      this.sp[hand[1]].to = sy + Math.sin(a) * 34;
    }

    makeItem(file = {}) {
      if (file.note) return this.makeNote();
      const name = file.name || '', count = file.count || 1;
      const ext = (file.ext || (name.includes('.') ? name.split('.').pop() : '')).slice(0, 4).toUpperCase();
      const g = el('g'), line = { stroke: '#b9c0ca', 'stroke-width': 1.2, 'stroke-linejoin': 'round' };
      if (count > 1) el('path', { d: PAGE, fill: '#eef1f5', ...line, transform: 'translate(5 -5)' }, g);
      el('path', { d: PAGE, fill: '#fff', ...line }, g);
      el('path', { d: 'M 6 -21 L 6 -12 L 15 -12', fill: '#e6eaf0', ...line }, g);
      el('line', { x1: -10, x2: 1, y1: -12, y2: -12, stroke: '#d3d9e1', 'stroke-width': 2, 'stroke-linecap': 'round' }, g);
      el('line', { x1: -10, x2: 8, y1: -6, y2: -6, stroke: '#d3d9e1', 'stroke-width': 2, 'stroke-linecap': 'round' }, g);
      el('rect', { x: -15, y: 2, width: 30, height: 12, fill: ext ? extColor(ext) : '#8a93a0' }, g);
      const tx = el('text', { x: 0, y: 11.2, 'text-anchor': 'middle', 'font-size': 8.5, 'font-weight': 800, fill: '#fff', 'font-family': FONT }, g);
      tx.textContent = ext || (count > 1 ? '×' + count : '•••');
      return g;
    }
    // Записка: жёлтый листок с текстом.
    makeNote() {
      const g = el('g');
      el('path', { d: 'M -16 -18 L 16 -18 L 16 12 L 8 20 L -16 20 Z', fill: '#ffe066', stroke: '#e0b400', 'stroke-width': 1.2, 'stroke-linejoin': 'round' }, g);
      el('path', { d: 'M 16 12 L 8 12 L 8 20', fill: '#f5c400', stroke: '#e0b400', 'stroke-width': 1.2, 'stroke-linejoin': 'round' }, g);
      for (const [y, w] of [[-10, 22], [-4, 26], [2, 18], [8, 12]]) el('line', { x1: -11, x2: -11 + w, y1: y, y2: y, stroke: '#b98a00', 'stroke-width': 1.8, 'stroke-linecap': 'round', opacity: 0.6 }, g);
      el('rect', { x: -6, y: -22, width: 12, height: 6, rx: 1, fill: 'rgba(255,255,255,.55)' }, g);   // скотч
      return g;
    }

    makeBox(label) {
      const g = el('g'), line = { stroke: '#8a5a2b', 'stroke-width': 1.5, 'stroke-linejoin': 'round' };
      el('rect', { x: -30, y: -20, width: 60, height: 40, rx: 3, fill: '#c98f55', ...line }, g);
      el('path', { d: 'M -30 -20 L -22 -27 L 22 -27 L 30 -20 Z', fill: '#dba56b', ...line }, g);
      el('rect', { x: -6, y: -27, width: 12, height: 47, fill: '#e9c48f', opacity: 0.9 }, g);
      el('rect', { x: -16, y: 1, width: 32, height: 13, rx: 2, fill: '#fff9ee', stroke: 'rgba(0,0,0,.15)', 'stroke-width': 0.8 }, g);
      const tx = el('text', { x: 0, y: 10.8, 'text-anchor': 'middle', 'font-size': 9, 'font-weight': 800, fill: '#5a3a1a', 'font-family': FONT }, g);
      tx.textContent = label;
      return g;
    }

    fx(node, life, fn) {
      this.n.fx.appendChild(node);
      const p = { node, life, age: 0, fn };
      this.parts.push(p);
      fn(0, 0);
    }
    plane(color, ref) {
      const g = el('g');
      const trail = el('path', { fill: 'none', stroke: color, 'stroke-width': 2.2, 'stroke-dasharray': '0.1 6', 'stroke-linecap': 'round', opacity: 0.85 }, g);
      const p = el('g', null, g), line = { stroke: '#8792a3', 'stroke-width': 1.2, 'stroke-linejoin': 'round' };
      el('path', { d: 'M -13 -6 L 16 0 L -13 7 L -7 0.5 Z', fill: '#fff', ...line }, p);
      el('path', { d: 'M -13 -6 L -7 0.5 L -13 7 Z', fill: color }, p);
      el('path', { d: 'M -7 0.5 L 16 0', fill: 'none', ...line }, p);
      const A = [0, -96], B = [-95, -175], C = [80, -235], D = [215, -130], pts = [];
      this.fx(g, 1.5, k => {
        const t = ease.inOut(k), q = bez(A, B, C, D, t), q2 = bez(A, B, C, D, Math.min(1, t + 0.01));
        ref.pos = q;
        pts.push(q); if (pts.length > 40) pts.shift();
        set(trail, 'd', 'M ' + pts.map(v => `${r2(v[0])} ${r2(v[1])}`).join(' L '));
        set(p, 'transform', `translate(${r2(q[0])} ${r2(q[1])}) rotate(${r2((Math.atan2(q2[1] - q[1], q2[0] - q[0]) * 180) / Math.PI)}) scale(${r2(1 + k * 0.3)})`);
        set(g, 'opacity', r2(1 - smooth(0.8, 1, k)));
      });
    }
    badge() {
      const g = el('g');
      el('circle', { r: 13, fill: '#2fb344', stroke: '#fff', 'stroke-width': 2.5 }, g);
      el('path', { d: 'M -6 0.5 L -1.5 5 L 6.5 -4.5', fill: 'none', stroke: '#fff', 'stroke-width': 3.2, 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, g);
      this.fx(g, 1.6, k => {
        set(g, 'transform', `translate(46 ${r2(-128 - k * 12)}) scale(${r2(k < 0.25 ? ease.back(k / 0.25) : 1)})`);
        set(g, 'opacity', r2(1 - smooth(0.75, 1, k)));
      });
    }
    bubble(txt, life = 1.4, x = 40) {
      const g = el('g');
      const tx = el('text', { 'text-anchor': 'middle', 'font-size': 34, 'font-weight': 900, fill: '#fff', stroke: this.pal.line, 'stroke-width': 6, 'stroke-linejoin': 'round', 'paint-order': 'stroke', 'font-family': FONT }, g);
      tx.textContent = txt;
      this.fx(g, life, k => {
        set(g, 'transform', `translate(${x} ${r2(-122 + this.jy * 0.7)}) rotate(${r2(Math.sin(this.t * 6) * 6)}) scale(${r2(k < 0.2 ? ease.back(k / 0.2) : 1)})`);
        set(g, 'opacity', r2(1 - smooth(0.82, 1, k)));
      });
    }
    sparkles(count = 3) {
      for (let i = 0; i < count; i++) {
        const g = el('path', { d: STAR, fill: i % 2 ? '#ffd43b' : '#fff3a3', stroke: '#f0a500', 'stroke-width': 0.8 });
        const x = rnd(-80, 80), y = rnd(-150, -60), sz = rnd(0.7, 1.3), delay = i * 0.12, life = 0.9 + delay;
        this.fx(g, life, k => {
          const kk = clamp((k * life - delay) / 0.9);
          set(g, 'transform', `translate(${r2(x)} ${r2(y - kk * 10)}) rotate(${r2(kk * 90)}) scale(${r2(Math.sin(kk * Math.PI) * sz)})`);
        });
      }
    }
    hearts(count = 3) {
      for (let i = 0; i < count; i++) {
        const g = el('path', { d: heartPath(8), fill: '#ff5d7d', stroke: '#d6335a', 'stroke-width': 1 });
        const x0 = rnd(-50, 50), delay = i * 0.25, life = 1.6 + delay;
        this.fx(g, life, k => {
          const kk = clamp((k * life - delay) / 1.6);
          set(g, 'transform', `translate(${r2(x0 + Math.sin(kk * 9 + i) * 8)} ${r2(-110 - kk * 60)}) scale(${r2(smooth(0, 0.15, kk) * (1 - kk * 0.3))})`);
          set(g, 'opacity', r2(1 - smooth(0.7, 1, kk)));
        });
      }
    }
    zzz() {
      const g = el('text', { 'font-size': 15, 'font-weight': 800, fill: this.pal.led || this.pal.line, 'font-family': FONT });
      g.textContent = 'z';
      this.fx(g, 2.8, k => {
        set(g, 'transform', `translate(${r2(28 + k * 40 + Math.sin(k * 8) * 4)} ${r2(-118 - k * 52)}) scale(${r2(0.6 + k)})`);
        set(g, 'opacity', r2(smooth(0, 0.15, k) * (1 - smooth(0.6, 1, k))));
      });
    }
    puff(x, y, size = 1, dx = 0, dy = -20) {
      const g = el('circle', { r: 6, fill: 'rgba(255,255,255,.92)', stroke: 'rgba(120,130,145,.35)', 'stroke-width': 1 });
      this.fx(g, 0.6, k => {
        const e = ease.out(k);
        set(g, 'transform', `translate(${r2(x + dx * e)} ${r2(y + dy * e)}) scale(${r2(size * (0.5 + e * 1.3))})`);
        set(g, 'opacity', r2(1 - k));
      });
    }
    dust() { this.puff(-40, -2, 0.8, -26, -6); this.puff(40, -2, 0.8, 26, -6); }
    flyPaper(i) {
      const g = el('g'), sh = el('g', null, g);
      el('rect', { x: -9, y: -11, width: 18, height: 22, rx: 1.5, fill: '#fffdf7', stroke: 'rgba(0,0,0,.18)', 'stroke-width': 0.8 }, sh);
      for (const y of [-6, -2, 2]) el('line', { x1: -5, x2: 5, y1: y, y2: y, stroke: '#cfd6e0', 'stroke-width': 1.2 }, sh);
      let x = rnd(-20, 20), y = -100, vx = rnd(-110, 110) + i * 10, vy = rnd(-230, -170), rot = rnd(-40, 40);
      const vr = rnd(-400, 400);
      this.fx(g, 1.5, (k, dt) => {
        vy += 330 * dt; x += vx * dt; y += vy * dt; rot += vr * dt;
        set(g, 'transform', `translate(${r2(x)} ${r2(y)}) rotate(${r2(rot)})`);
        set(g, 'opacity', r2(1 - smooth(0.7, 1, k)));
      });
    }
    stars(dur) {
      for (let i = 0; i < 3; i++) {
        const g = el('path', { d: STAR, fill: '#ffd43b', stroke: '#e09b00', 'stroke-width': 0.8 });
        this.fx(g, dur, k => {
          const a = this.t * 5 + (i * TAU) / 3;
          set(g, 'transform', `translate(${r2(Math.cos(a) * 46)} ${r2(-122 + Math.sin(a) * 9 + this.jy)}) scale(${r2((0.75 + Math.sin(a) * 0.25) * (1 - smooth(0.85, 1, k)))})`);
        });
      }
    }
  }

  // Один общий цикл кадров на всех Папычей; в скрытом окне браузер его сам останавливает.
  // Окно спрятано (шторка уехала) — программа выключает цикл, чтобы не тратить процессор.
  let last = 0, running = false, active = true;
  function loop(ts) {
    const dt = Math.min(1 / 30, last ? (ts - last) / 1000 : 1 / 60);
    last = ts;
    for (const p of all) p.tick(dt);
    if (all.size && active) requestAnimationFrame(loop); else running = false;
  }
  function start() { if (!running && active) { running = true; last = 0; requestAnimationFrame(loop); } }

  // Курсор: в окне — события мыши; для шторки программа будет передавать положение курсора на экране.
  function cursor(x, y) {
    input.x = x; input.y = y; input.t = now();
    for (const p of all) p.wake();
  }
  addEventListener('pointermove', e => cursor(e.clientX, e.clientY), { passive: true });
  addEventListener('dragover', e => cursor(e.clientX, e.clientY));
  document.documentElement.addEventListener('pointerleave', () => { input.x = null; });

  // У каждого устройства семьи свой цвет глаз — по коду устройства, одинаковый в окне и шторке.
  const HUES = [28, 330, 140, 265, 48, 205, 0, 170];
  Papych.hueOf = id => {
    let h = 0;
    for (const c of String(id)) h = (h * 31 + c.charCodeAt(0)) >>> 0;
    return HUES[h % HUES.length];
  };
  Papych.colorOf = id => `hsl(${Papych.hueOf(id)},100%,62%)`;

  Papych.cursor = cursor;
  Papych.active = on => { active = on; if (on) start(); };
  Papych.variants = Object.keys(STYLES);
  window.Papych = Papych;
})();
