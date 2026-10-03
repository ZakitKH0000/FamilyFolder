// Управление окном программы через отладочный порт WebView2 (без мыши).
// Программу запустить с WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<порт>.
//   node scripts/cdp.js <порт> eval "<js>"      — выполнить JS в окне, вывести результат
//   node scripts/cdp.js <порт> shot <файл.png>  — снимок содержимого окна
//   node scripts/cdp.js <порт> click "x,y"      — нажатие мышью в окне (без движения настоящей мыши)
// Последним словом можно выбрать окно: island — шторка, tour — экскурсия (по умолчанию — окно программы).
const [port, cmd, arg, which = 'main'] = process.argv.slice(2);

async function main() {
  const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
  const pages = list.filter(p => p.type === 'page');
  const page = pages.find(p => /tauri\.localhost/.test(p.url) && (which === 'main' ? !/island|tour/.test(p.url) : p.url.includes(which))) || pages.find(p => /tauri\.localhost/.test(p.url)) || pages[0];
  if (!page) throw new Error('окно не найдено');
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((ok, fail) => { ws.onopen = ok; ws.onerror = fail; });
  let id = 0;
  const pending = new Map();
  ws.onmessage = m => {
    const msg = JSON.parse(m.data);
    if (msg.id && pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); }
  };
  const call = (method, params = {}) => new Promise(res => { const i = ++id; pending.set(i, res); ws.send(JSON.stringify({ id: i, method, params })); });

  if (cmd === 'eval') {
    const r = await call('Runtime.evaluate', { expression: arg, awaitPromise: true, returnByValue: true });
    if (r.result?.exceptionDetails) console.log('ОШИБКА:', JSON.stringify(r.result.exceptionDetails.exception?.description || r.result.exceptionDetails));
    else console.log(typeof r.result?.result?.value === 'string' ? r.result.result.value : JSON.stringify(r.result?.result?.value, null, 1));
  } else if (cmd === 'click') {
    // node scripts/cdp.js <порт> click "x,y" — нажатие мышью внутри окна (открывает и списки Windows)
    const [x, y] = arg.split(',').map(Number);
    for (const type of ['mousePressed', 'mouseReleased']) {
      await call('Input.dispatchMouseEvent', { type, x, y, button: 'left', clickCount: 1 });
    }
    console.log('нажато:', x, y);
  } else if (cmd === 'shot') {
    const r = await call('Page.captureScreenshot', { format: 'png' });
    require('fs').writeFileSync(arg, Buffer.from(r.result.data, 'base64'));
    console.log('снимок:', arg);
  }
  ws.close();
}
main().catch(e => { console.error(String(e)); process.exit(1); });
