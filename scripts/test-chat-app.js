'use strict';
// Два изолированных WebView2/iroh-экземпляра. Микрофон заменён синтетическим тоном;
// пользовательский звук, семейные устройства и папки тест не затрагивает.
const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'family-chat-test-'));
const exe = process.env.OBSHAYA_TEST_EXE || path.resolve(__dirname, '../target/debug/ObshayaPapka.exe');
const children = [], clients = [];
const sleep = ms => new Promise(r => setTimeout(r, ms));
async function until(label, predicate, seconds = 45) {
  const end = Date.now() + seconds * 1000;
  while (Date.now() < end) { const value = await predicate(); if (value) return value; await sleep(250); }
  throw new Error('Timeout: ' + label);
}
async function attach(page) {
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((ok, fail) => { ws.onopen = ok; ws.onerror = fail; });
  const errors=[];
  let serial = 0; const pending = new Map();
  ws.onmessage = e => { const m = JSON.parse(e.data); if (m.method==='Runtime.exceptionThrown') errors.push(m.params.exceptionDetails.exception?.description||m.params.exceptionDetails.text); if (m.id) { pending.get(m.id)?.(m); pending.delete(m.id); } };
  const call = (method, params) => new Promise(resolve => { const id = ++serial; pending.set(id, resolve); ws.send(JSON.stringify({ id, method, params })); });
  const evalJS = async expression => {
    const r = await call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true, userGesture: true });
    if (r.result?.exceptionDetails) throw new Error(r.result.exceptionDetails.exception?.description || 'JS exception');
    return r.result?.result?.value;
  };
  clients.push(ws);
  await call('Runtime.enable',{});
  return {eval:evalJS,call,errors};
}
async function start(name, port) {
  assert.ok(fs.existsSync(exe), 'build obshaya-app first');
  const child = spawn(exe, [], { windowsHide: true, stdio: 'ignore', env: { ...process.env,
    OBSHAYA_DATA_DIR: path.join(root, name + '-data'), OBSHAYA_FOLDER: path.join(root, name + '-folder'),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=' + port + ' --use-fake-device-for-media-stream --use-fake-ui-for-media-stream' } });
  children.push(child);
  const page = await until('WebView ' + name, async () => {
    try { return (await (await fetch('http://127.0.0.1:' + port + '/json')).json()).find(p => p.type === 'page' && /tauri\.localhost/.test(p.url) && !/island|tour/.test(p.url)); } catch { return false; }
  });
  const connected = await attach(page);
  const {eval:evalJS,call}=connected;
  await until('UI init ' + name, () => evalJS("typeof S !== 'undefined' && S && document.querySelector('#chat-peer') !== null"));
  assert.equal(await evalJS("invoke('get_state').then(s=>s.folder)"), path.join(root, name + '-folder'),
    'debug port must belong to this isolated test instance before any changes');
  await evalJS(`(async()=>{ await invoke('save_settings',{settings:{...S.settings, device_name:${JSON.stringify(name)},onboarded:true,tour_seen:999,tips:false,dock_panel:false}}); S=await invoke('get_state'); closeViews(); render(); switchTab('chat'); })()`);
  return { eval: evalJS, call, errors:connected.errors, port, state: () => evalJS("invoke('get_state')") };
}
async function main() {
  const a = await start('Chat A', 9381), b = await start('Chat B', 9382);
  const code = await a.eval("invoke('create_invite')");
  await b.eval(`invoke('join',{code:${JSON.stringify(code)}})`);
  await until('peers online', async () => (await a.state()).peers.some(p => p.online) && (await b.state()).peers.some(p => p.online));
  const bid = (await a.state()).peers[0].id;
  await a.eval(`(async()=>{ S=await invoke('get_state'); render(); Chat.select(${JSON.stringify(bid)}); document.querySelector('#chat-text').value='native chat test'; document.querySelector('#chat-send').click(); })()`);
  await until('text delivered', async () => (await b.state()).notes.some(n => n.text === 'native chat test' && !n.group));
  assert.equal((await b.state()).notes.filter(n => n.text === 'native chat test').length, 1);
  const original = (await a.state()).notes.find(n => n.text === 'native chat test');
  assert.equal(await a.eval("document.querySelectorAll('#buddy .papych').length"), 1);
  assert.equal(await a.eval("!!document.querySelector('#chat-buddy')"), false);
  assert.equal(await a.eval("Math.round(document.querySelector('#buddy').getBoundingClientRect().width)"), 78);
  await a.eval(`document.querySelector('[data-chat=reply][data-id="${original.id}"]').click()`);
  assert.equal(await a.eval("!document.querySelector('#chat-reply').classList.contains('hidden')"), true);
  // Реальные MediaRecorder, Blob, AudioContext и CSP; только источник звука тестовый.
  assert.equal(await a.eval("navigator.mediaDevices.enumerateDevices().then(ds=>ds.filter(d=>d.kind==='audioinput').every(d=>d.label.toLowerCase().includes('fake')) && ds.some(d=>d.kind==='audioinput'))"), true,
    'fake browser microphone is mandatory: never capture the user microphone');
  await a.eval(`window.__chatTest = {tracks:[],contexts:[]}; window.__realMic=navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
    navigator.mediaDevices.getUserMedia=async constraints=>{const s=await __realMic(constraints); __chatTest.tracks.push(...s.getTracks()); return s;}; document.querySelector('#chat-mic').click();`);
  await until('recording', () => a.eval("!!document.querySelector('#chat-clock')"));
  await sleep(1600);
  assert.equal(await a.eval("document.querySelector('#buddy').classList.contains('recording') && document.querySelector('#chat-mic').classList.contains('pressed')"), true, 'robot holds recording button');
  await a.eval("document.querySelector('[data-record=stop]').click()");
  await until('preview', () => a.eval("!!document.querySelector('#chat-record audio')"));
  assert.equal(await a.eval("__chatTest.tracks.every(t=>t.readyState==='ended')"), true, 'mic released before preview');
  await a.eval("window.__preview=document.querySelector('#chat-record audio'); __preview.muted=true; __preview.play()");
  await a.eval("document.querySelector('#chat-send').click()");
  const voice = await until('voice delivered', async () => (await b.state()).notes.find(n => n.voice && n.voice_ready));
  assert.equal(voice.group, false);
  assert.equal(voice.reply_to, original.id);
  assert.equal(voice.voice.waveform.length, 48);
  assert.ok(voice.voice.waveform.some(v => v > 0), 'waveform records real synthetic sound levels');
  assert.ok(voice.voice.size > 100);
  assert.equal(await a.eval("__preview.paused"), true, 'sending stops preview');
  // Декодирование и воспроизведение на получателе без звука из динамиков пользователя.
  await b.eval(`(async()=>{ S=await invoke('get_state'); render(); window.__NativeAudio=Audio; window.Audio=function(url){const a=new __NativeAudio(url); a.muted=true; window.__testAudio=a; return a;};
    Chat.select(${JSON.stringify((await b.state()).peers[0].id)}); document.querySelector('[data-chat=play]').click(); })()`);
  await until('voice decoded', () => b.eval("window.__testAudio && !__testAudio.paused && __testAudio.readyState>=2"));
  assert.equal(await b.eval("document.querySelector('#buddy').classList.contains('speaking')"), true, 'Papych accompanies playback');
  await until('robot reaches voice', () => b.eval("document.querySelector('.voice-play').classList.contains('pressed')"));
  assert.equal(await b.eval("!!document.querySelector('.chat-quote') && document.querySelectorAll('.wave-bars i').length===48"), true);
  await b.eval("document.querySelector('[data-chat=play]').click()");
  assert.equal(await b.eval("__testAudio.paused"), true, 'voice pauses');
  await b.eval("document.querySelector('[data-chat=play]').click()");
  const seekTime = await b.eval("const seek=document.querySelector('[data-voice]'); seek.value='500'; seek.dispatchEvent(new Event('input')); __testAudio.currentTime");
  assert.ok(Math.abs(seekTime - voice.voice.duration_ms / 2000) < .2, 'seek works with a MediaRecorder WebM duration');
  await b.eval("Chat.stop()");
  assert.equal(await b.eval("__testAudio.paused"), true, 'closing chat stops playback');
  await until('robot returns to shelf', () => b.eval("!document.querySelector('#buddy').classList.contains('chat-visiting') && getComputedStyle(document.querySelector('#buddy')).transform==='none'"));
  const cleanShot = await b.call('Page.captureScreenshot', {format:'png'});
  fs.writeFileSync(path.join(root, 'chat.png'), Buffer.from(cleanShot.result.data, 'base64'));
  // Наведение отправляется только внутри изолированного WebView через CDP: курсор ОС не двигается.
  await b.eval("document.activeElement.blur()");
  await b.call('Input.dispatchMouseEvent', {type:'mouseMoved', x:5, y:5});
  await until('actions hidden', () => b.eval("getComputedStyle(document.querySelector('.chat-actions')).opacity==='0'"));
  const point = await b.eval("const r=document.querySelector('.chat-message').getBoundingClientRect(); ({x:r.left+r.width/2,y:r.top+r.height/2})");
  await b.call('Input.dispatchMouseEvent', {type:'mouseMoved', ...point});
  await until('hover actions', () => b.eval("getComputedStyle(document.querySelector('.chat-actions')).opacity==='1'"));
  const menuShot = await b.call('Page.captureScreenshot', {format:'png'});
  fs.writeFileSync(path.join(root, 'chat-actions.png'), Buffer.from(menuShot.result.data, 'base64'));
  assert.equal(await b.eval("document.querySelector('.chat-message [data-chat=copy]').title===t('btn.copy') && !!document.querySelector('.chat-message [data-chat=reply]')"), true);
  await b.eval("document.querySelector('.chat-message [data-chat=reply]').click(); document.querySelector('#chat-text').value='native reply test'; document.querySelector('#chat-send').click()");
  const textReply = await until('text reply delivered', async () => (await a.state()).notes.find(n => n.text === 'native reply test'));
  assert.equal(textReply.reply_to, original.id);
  await b.call('Input.dispatchMouseEvent', {type:'mouseMoved', x:5, y:5});
  const islandPage = await until('island page',async()=> (await (await fetch('http://127.0.0.1:'+a.port+'/json')).json()).find(p=>/island.html/.test(p.url)));
  const mini=await attach(islandPage);
  await until('mini init',()=>mini.eval("typeof MiniChat!=='undefined' && S && !!document.querySelector('#chat-peer')"));
  assert.equal(await mini.eval("invoke('get_state').then(s=>s.folder)"),path.join(root,'Chat A-folder'));
  await a.eval("invoke('island_demo',{kind:'peek'})");
  await until('peek chat button',()=>mini.eval("mode==='peek' && [...document.querySelectorAll('#acts button')].some(b=>b.title===t('chat.title'))"));
  await mini.eval("[...document.querySelectorAll('#acts button')].find(b=>b.title===t('chat.title')).click()");
  await until('mini expanded',()=>mini.eval("mode==='chat' && !document.querySelector('#chat').classList.contains('hidden')"));
  assert.equal(await mini.eval("document.querySelector('#isl').getBoundingClientRect().bottom <= innerHeight"),true,'mini chat fits native window');
  await mini.eval(`Chat.select(${JSON.stringify(bid)}); document.querySelector('#chat-text').value='mini text test'; document.querySelector('#chat-send').click()`);
  await until('mini text delivered',async()=> (await b.state()).notes.some(n=>n.text==='mini text test'&&!n.group));
  assert.equal(await mini.eval("navigator.mediaDevices.enumerateDevices().then(ds=>ds.some(d=>d.kind==='audioinput')&&ds.filter(d=>d.kind==='audioinput').every(d=>d.label.toLowerCase().includes('fake')))"),true);
  await mini.eval("document.querySelector('#chat-mic').click()");
  await until('mini recording',()=>mini.eval("!!document.querySelector('#chat-clock')"));
  await sleep(1600);
  assert.equal(await mini.eval("document.querySelector('#chat-mic').classList.contains('pressed')"),true);
  await mini.eval("document.querySelector('[data-record=stop]').click()");
  await until('mini voice preview',()=>mini.eval("!!document.querySelector('#chat-record audio')"));
  await mini.eval("document.querySelector('#chat-send').click()");
  await until('mini voice delivered',async()=> (await b.state()).notes.filter(n=>n.voice&&n.voice_ready).length===2);
  const miniShot=await mini.call('Page.captureScreenshot',{format:'png'});
  fs.writeFileSync(path.join(root,'mini-chat.png'),Buffer.from(miniShot.result.data,'base64'));
  await mini.eval("close();");
  assert.equal(await mini.eval("document.querySelector('#chat').classList.contains('hidden')"),true);
  await mini.eval(`open('note',{id:${JSON.stringify(textReply.id)},from:'Chat B',text:'native reply test'}); document.querySelector('#acts button').click()`);
  await until('quick reply inside island',()=>mini.eval(`mode==='chat' && Chat.target()===${JSON.stringify(bid)} && !document.querySelector('#chat-reply').classList.contains('hidden')`));
  await mini.eval("Chat.select(''); document.querySelector('#chat-text').value='mini family test'; document.querySelector('#chat-send').click()");
  await until('mini family delivered',async()=> (await b.state()).notes.some(n=>n.text==='mini family test'&&n.group));
  await mini.eval("window.__miniTracks=[]; const real=navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices); navigator.mediaDevices.getUserMedia=async c=>{const s=await real(c); __miniTracks.push(...s.getTracks()); return s;}; document.querySelector('#chat-mic').click()");
  await until('mini recording before close',()=>mini.eval("!!document.querySelector('#chat-clock')"));
  await mini.eval("close()");
  await until('mini close releases mic',()=>mini.eval("__miniTracks.length>0 && __miniTracks.every(t=>t.readyState==='ended') && !document.querySelector('#chat-record audio')"));
  // Отмена активной записи и запоздалое разрешение: после ухода из чата микрофон выключен.
  await a.eval("document.querySelector('#chat-mic').click()");
  await until('recording again', () => a.eval("!!document.querySelector('#chat-clock')"));
  await a.eval("switchTab('in')");
  await until('cancel releases mic', () => a.eval("__chatTest.tracks.every(t=>t.readyState==='ended') && !document.querySelector('#chat-record audio')"));
  await a.eval("switchTab('chat'); window.__resolveMic=null; navigator.mediaDevices.getUserMedia=()=>new Promise(r=>{window.__resolveMic=r}); document.querySelector('#chat-mic').click();");
  await a.eval("switchTab('in'); switchTab('chat'); const ctx=new AudioContext(), dst=ctx.createMediaStreamDestination(); __chatTest.contexts.push(ctx); __chatTest.tracks.push(...dst.stream.getTracks()); __resolveMic(dst.stream);");
  await until('late permission cancelled', () => a.eval("__chatTest.tracks.every(t=>t.readyState==='ended') && !document.querySelector('#chat-clock')"));
  await until('mic available', () => a.eval("!document.querySelector('#chat-mic').disabled"));
  await a.eval("navigator.mediaDevices.getUserMedia=async()=>{throw new DOMException('test denial','NotAllowedError')}; document.querySelector('#chat-mic').click()");
  await until('permission denial explained', () => a.eval("document.querySelector('#toast').textContent===t('chat.mic_error') && !document.querySelector('#chat-mic').disabled"));
  for (const p of [a, b]) await p.eval("__chatTest?.contexts?.forEach(c=>c.close())").catch(() => {});
  const shot = await b.call('Page.captureScreenshot', { format: 'png' });
  fs.writeFileSync(path.join(root, 'reply.png'), Buffer.from(shot.result.data, 'base64'));
  for(const p of [a,b,mini]) assert.deepEqual(p.errors,[],'WebView JS exceptions');
  console.log('Native chat + island mini chat: text/reply, waveform, shared robot jumps/return, audio codec/CSP, playback/pause/seek, cancel and late permission passed');
  console.log('Test data and screenshot: ' + root);
}
main().catch(e => { console.error(e); process.exitCode = 1; }).finally(() => {
  clients.forEach(ws => ws.close()); children.forEach(p => p.kill());
});
