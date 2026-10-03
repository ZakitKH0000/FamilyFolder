// Снимки и анимации для страницы на GitHub — без окна программы, со страниц `node scripts/ui-preview.js`.
// Страница открывается в невидимом Edge; время в ней (JS и CSS) замедлено, кадры пишутся в папку,
// а собирает их в анимацию scripts/showcase.py. Сценки — design/showcase.html.
//   node scripts/showcase.js <адрес> <папка или файл.png> <ширина>x<высота> [--still] [--js "<код>"]
//        [--wait 2] [--rate 0.25] [--max 40] [--dpr 2] [--light] [--transparent]
// Анимация: страница получает window.__go() и ставит window.__done = true, когда сценка закончилась.
const { spawn } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');

const [url, out, size, ...rest] = process.argv.slice(2);
const opt = { rate: 0.25, max: 40, dpr: 2, wait: 2, still: false, light: false, transparent: false, js: '', probe: '' };
for (let i = 0; i < rest.length; i++) {
  const k = rest[i].replace(/^--/, '');
  if (k === 'still' || k === 'light' || k === 'transparent') opt[k] = true;
  else if (k === 'js' || k === 'probe') opt[k] = rest[++i];
  else opt[k] = Number(rest[++i]);
}
if (opt.still) opt.rate = 1;
const [W, H] = size.split('x').map(Number);
const EDGE = 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe';
const PORT = 9420;

// Замедление времени для JS: часы, кадры и таймеры идут в R раз медленнее (CSS замедляет Animation.setPlaybackRate).
const TIME = R => `(() => {
  const R = ${R};
  if (R === 1) return;
  const pn = performance.now.bind(performance), t0 = pn();
  const scale = t => t0 + (t - t0) * R;
  Object.defineProperty(performance, 'now', { value: () => scale(pn()), configurable: true });
  const dn = Date.now, d0 = dn();
  Date.now = () => Math.round(d0 + (dn() - d0) * R);
  const raf = requestAnimationFrame.bind(window);
  // Метку кадра — по своим часам: замедление CSS (Animation.setPlaybackRate) замедляет и метки браузера.
  window.requestAnimationFrame = fn => raf(() => fn(performance.now()));
  const st = setTimeout.bind(window), si = setInterval.bind(window);
  window.setTimeout = (fn, ms = 0, ...a) => st(fn, ms / R, ...a);
  window.setInterval = (fn, ms = 0, ...a) => si(fn, ms / R, ...a);
})();`;

const sleep = ms => new Promise(r => setTimeout(r, ms));

async function main() {
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'showcase-edge-'));
  const edge = spawn(EDGE, ['--headless=new', `--remote-debugging-port=${PORT}`, `--user-data-dir=${profile}`,
    '--hide-scrollbars', '--mute-audio', '--no-first-run', '--disable-extensions', `--window-size=${W},${H}`, 'about:blank'], { stdio: 'ignore' });
  try {
    let pages = null;
    for (let i = 0; i < 50 && !pages; i++) {
      await sleep(200);
      try { pages = (await (await fetch(`http://127.0.0.1:${PORT}/json`)).json()).filter(p => p.type === 'page'); } catch { /* ещё запускается */ }
    }
    const ws = new WebSocket(pages[0].webSocketDebuggerUrl);
    await new Promise((ok, fail) => { ws.onopen = ok; ws.onerror = fail; });
    let id = 0;
    const pending = new Map(), on = {};
    ws.onmessage = m => {
      const msg = JSON.parse(m.data);
      if (msg.id && pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); }
      else if (msg.method) (on[msg.method] || []).forEach(f => f(msg.params));
    };
    const call = (method, params = {}) => new Promise((res, fail) => {
      const i = ++id;
      pending.set(i, m => (m.error ? fail(new Error(`${method}: ${m.error.message}`)) : res(m.result)));
      ws.send(JSON.stringify({ id: i, method, params }));
    });
    const evaluate = async expr => (await call('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true })).result?.value;

    await call('Page.enable');
    await call('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: opt.dpr, mobile: false });
    await call('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: opt.light ? 'light' : 'dark' }] });
    if (opt.transparent) await call('Emulation.setDefaultBackgroundColorOverride', { color: { r: 0, g: 0, b: 0, a: 0 } });
    await call('Page.addScriptToEvaluateOnNewDocument', { source: TIME(opt.rate) });
    await call('Animation.enable');
    const loaded = new Promise(r => (on['Page.loadEventFired'] = [r]));
    await call('Page.navigate', { url });
    await loaded;
    await sleep(800);

    if (opt.still) {
      if (opt.js) await evaluate(opt.js);
      await sleep(opt.wait * 1000);
      const r = await call('Page.captureScreenshot', { format: 'png' });
      fs.writeFileSync(out, Buffer.from(r.data, 'base64'));
      console.log('снимок:', out);
      return;
    }

    fs.rmSync(out, { recursive: true, force: true });
    fs.mkdirSync(out, { recursive: true });
    const frames = [];
    on['Page.screencastFrame'] = [p => {
      frames.push({ data: p.data, ts: p.metadata.timestamp });
      call('Page.screencastFrameAck', { sessionId: p.sessionId }).catch(() => {});
    }];
    const slow = () => call('Animation.setPlaybackRate', { playbackRate: opt.rate }).catch(() => {});
    await slow();
    await call('Page.startScreencast', { format: 'png', everyNthFrame: 1 });
    await sleep(300);
    await evaluate('window.__go && void window.__go(), true');
    const until = Date.now() + (opt.max / opt.rate) * 1000;
    // Новые рамки (iframe) получают свою шкалу CSS — замедлять повторно, пока идёт запись.
    while (Date.now() < until && !(await evaluate('!!window.__done'))) {
      await slow();
      if (opt.probe) console.log(JSON.stringify(await evaluate(opt.probe)));
      await sleep(150);
    }
    await call('Page.stopScreencast');
    await sleep(300);
    const t0 = frames[0].ts;
    const list = frames.map((f, i) => {
      const name = `f${String(i).padStart(5, '0')}.png`;
      fs.writeFileSync(path.join(out, name), Buffer.from(f.data, 'base64'));
      return { name, t: (f.ts - t0) * opt.rate };
    });
    fs.writeFileSync(path.join(out, 'frames.json'), JSON.stringify(list));
    console.log(`кадров: ${list.length}, длительность: ${list[list.length - 1].t.toFixed(2)} с →`, out);
  } finally {
    edge.kill();
    await sleep(500);
    try { fs.rmSync(profile, { recursive: true, force: true }); } catch { /* Edge ещё держит файлы */ }
  }
}
main().catch(e => { console.error(String(e)); process.exit(1); });
