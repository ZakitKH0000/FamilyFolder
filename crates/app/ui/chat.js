'use strict';
// Отдельные разговоры. Запись требует явного нажатия и разрешения на микрофон.
const Chat = (() => {
  let peer = '', bot, host, signature = '', lastIds = new Set(), delivered = new Map(), shown = false;
  let draft = new Map(), recorder = null, stream = null, audioCtx = null, analyser = null;
  let chunks = [], recordingAt = 0, clock = null, pending = null, sending = false, discard = false;
  let player = null, playerUrl = null, playId = '', meter = null, micPending = false;
  let seenTimer = 0, recordPeer = '', previewUrl = null, playToken = 0, micToken = 0, replyTo = null, recordLevels = [];
  const replies = new Map(), cloudStates = new Map();
  const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const fmt = ms => `${Math.floor(ms / 60000)}:${String(Math.floor(ms / 1000) % 60).padStart(2, '0')}`;
  const conversation = n => n.group === undefined ? (!n.outgoing || n.to.length !== 1) : n.group;
  const matches = (n, target = peer) => target
    ? !conversation(n) && (n.outgoing ? n.to.some(p => p.id === target) : n.peer_id === target)
    : conversation(n);
  let embedded = false, presence = '';
  function init(options = {}) {
    embedded = !!options.embedded;
    host = document.querySelector('#chat');
    host.innerHTML = `<div class="chat-head"><div class="chat-identity"><label for="chat-peer"></label>
      <select id="chat-peer"></select><small id="chat-presence"></small></div>
      </div>
      <div class="chat-history" id="chat-history" role="log" aria-live="polite"></div>
      <div class="chat-compose"><div id="chat-reply" class="hidden"></div><div id="chat-record" class="hidden" aria-live="polite"></div>
      <textarea id="chat-text" maxlength="20000" rows="2"></textarea>
      <div class="chat-controls"><button class="btn" id="chat-mic"></button><span id="chat-hint"></span>
      <button class="btn primary" id="chat-send"></button></div></div>`;
    bot = Buddy.actor();
    host.querySelector('#chat-peer').onchange = e => select(e.target.value);
    host.querySelector('#chat-text').oninput = e => { draft.set(peer, e.target.value); Buddy.watch(e.target); };
    host.querySelector('#chat-text').onkeydown = e => {
      if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); send(); }
    };
    host.querySelector('#chat-send').onclick = send;
    host.querySelector('#chat-reply').onclick = e => e.target.closest('button') ? setReply(null) : jumpTo(replyTo);
    host.querySelector('#chat-mic').onclick = () => recorder ? finishRecording() : record();
    host.querySelector('#chat-record').onclick = e => {
      const action = e.target.closest('[data-record]')?.dataset.record;
      if (action === 'cancel') cancelRecording();
      if (action === 'stop') finishRecording();
    };
    host.querySelector('#chat-history').onclick = async e => {
      const btn = e.target.closest('[data-chat]');
      if (!btn) return;
      const n = S.notes.find(n => n.id === btn.dataset.id);
      if (!n) return;
      try {
        if (btn.dataset.chat === 'play') await play(n, btn);
        if (btn.dataset.chat === 'reply') setReply(n.id);
        if (btn.dataset.chat === 'jump') jumpTo(n.id);
        if (btn.dataset.chat === 'copy') { await invoke('copy_text', { text: n.text }); bot.boop(); }
        if (btn.dataset.chat === 'link') await invoke('open_url', { url: btn.dataset.url });
        if (btn.dataset.chat === 'delete' && await confirmDialog(t('chat.delete'), t('chat.delete_hint'), t('chat.delete'), true)) {
          if (playId === n.id) stopPlayback();
          await invoke('dismiss', { id: n.id });
          S = await invoke('get_state'); update();
        }
      } catch (e) { toast(esc(String(e)), 'err'); }
    };
    host.querySelector('#chat-history').onscroll = markSeen;
    host.querySelector('#chat-history').oncontextmenu = e => {
      const message = e.target.closest('[data-message]'); if (!message) return;
      e.preventDefault(); host.querySelectorAll('.menu-open').forEach(n => n.classList.remove('menu-open'));
      message.classList.add('menu-open');
    };
    document.addEventListener('pointerdown', e => { if (!e.target.closest('[data-message]')) host.querySelectorAll('.menu-open').forEach(n => n.classList.remove('menu-open')); });
    if (!embedded) T.event.listen('papych-active', e => { if (!e.payload) stop(); else markSeen(); });
    if (!embedded) T.event.listen('show-chat', e => { closeViews(); select(e.payload || ''); switchTab('chat'); });
    addEventListener('focus', syncPresence); addEventListener('blur', syncPresence);
    addEventListener('visibilitychange', () => { if (document.hidden) stop(); });
  }
  function select(id) {
    if (!host || sending || micPending) return;
    draft.set(peer, host.querySelector('#chat-text').value); replies.set(peer, replyTo);
    stop(); peer = id; replyTo = replies.get(peer) || null; signature = '';
    host.querySelector('#chat-text').value = draft.get(peer) || '';
    update();
    host.querySelector('#chat-history').scrollTop = 1e9;
  }
  function show(on) { shown = on; if (!on) stop(); else update(); }
  function compose(text) {
    if (!host) return;
    if (text) { host.querySelector('#chat-text').value = text; draft.set(peer, text); }
    host.querySelector('#chat-text').focus();
  }
  function status(n) {
    if (!n.outgoing) return '';
    if (n.cloud_error) return t('chat.cloud_error');
    if (n.cloud_ready) return t('chat.cloud_ready');
    if (n.to.every(p => p.delivered)) return t('chat.delivered');
    if (n.to.some(p => p.needs_update)) return t('chat.update_needed');
    return n.to.some(p => !p.delivered && !S.peers.some(x => x.id === p.id && x.online))
      ? t('chat.waiting') : t('chat.sending');
  }
  function replyLabel(n) { return n.voice ? t('chat.voice') : n.text; }
  function setReply(id) {
    replyTo = id; replies.set(peer, id); renderReply();
    if (id) host.querySelector('#chat-text').focus();
  }
  function renderReply() {
    const area = host.querySelector('#chat-reply');
    const n = S.notes.find(n => n.id === replyTo && matches(n));
    if (!n) { replyTo = null; area.classList.add('hidden'); area.innerHTML = ''; return; }
    area.classList.remove('hidden');
    area.innerHTML = `<div><b>${t('chat.reply')} · ${esc(n.outgoing ? S.my_name : n.peer)}</b>
      <span>${esc(replyLabel(n).slice(0, 160))}</span></div><button class="btn subtle sm" aria-label="${t('btn.cancel')}">${icon('close')}</button>`;
  }
  function jumpTo(id) {
    const node = host.querySelector(`[data-message="${CSS.escape(id)}"]`);
    if (!node) return;
    node.scrollIntoView({ block: 'center', behavior: reduce ? 'instant' : 'smooth' });
    if (!reduce) node.animate([{ filter: 'brightness(1.5)' }, { filter: 'none' }], { duration: 650 });
  }
  function waveform(n) {
    const bars = n.voice.waveform?.length ? n.voice.waveform : Array(48).fill(0);
    return bars.map(v => `<i style="height:${3 + Math.round(v / 255 * 24)}px"></i>`).join('');
  }
  function bubble(n) {
    const id = esc(n.id), voice = n.voice;
    const quote = S.notes.find(x => x.id === n.reply_to && matches(x));
    const reply = n.reply_to ? `<button class="chat-quote" ${quote ? `data-chat="jump" data-id="${esc(quote.id)}"` : 'disabled'}>
      <b>${esc(quote ? quote.outgoing ? S.my_name : quote.peer : t('chat.reply'))}</b>
      <span>${esc(quote ? replyLabel(quote).slice(0, 140) : t('chat.reply_missing'))}</span></button>` : '';
    const controls = voice ? `<div class="voice-player">
      <button class="voice-play" data-chat="play" data-id="${id}" ${!n.voice_ready ? 'disabled' : ''} aria-label="${t(playId === n.id && player && !player.paused ? 'chat.pause' : 'chat.play')}">
        ${icon(playId === n.id && player && !player.paused ? 'pause' : n.voice_ready ? 'play' : 'arrowDown')}</button>
      <div class="voice-track"><div class="voice-wave" data-wave="${id}"><div class="wave-bars">${waveform(n)}</div>
        <input class="voice-seek" type="range" min="0" max="1000" value="0" data-voice="${id}" ${!n.voice_ready ? 'disabled' : ''} aria-label="${t('chat.seek')}"></div>
        <div class="voice-caption"><time data-elapsed="${id}">${fmt(Math.ceil(voice.duration_ms / 1000) * 1000)}</time>
        <span>${t(n.voice_ready ? 'chat.voice' : 'chat.voice_loading')}</span></div></div></div>`
      : `<div class="chat-text">${linkify(n.text)}</div>`;
    const time = new Date(n.created_at).toLocaleTimeString(L._lang, { hour: '2-digit', minute: '2-digit' });
    const done = n.outgoing && n.to.every(p => p.delivered);
    return `<article class="chat-message ${n.outgoing ? 'mine' : 'theirs'}" data-message="${id}" tabindex="0">
      <div class="chat-actions" role="toolbar" aria-label="${t('chat.actions')}">
        <button class="btn subtle sm" data-chat="reply" data-id="${id}" title="${t('chat.reply')}">${icon('reply')}</button>
        ${!voice ? `<button class="btn subtle sm" data-chat="copy" data-id="${id}" title="${t('btn.copy')}">${icon('copy')}</button>` : ''}
        <button class="btn subtle sm" data-chat="delete" data-id="${id}" title="${t('chat.delete')}">${icon('trash')}</button></div>
      ${!n.outgoing && !peer ? `<strong>${esc(n.peer)}</strong>` : ''}${reply}${controls}
      ${n.cloud_ready || n.cloud_error ? `<small class="chat-cloud ${n.cloud_error ? 'err' : ''}">${icon(n.cloud_error ? 'warn' : 'cloud')}${esc(status(n))}</small>` : ''}<footer><time>${esc(time)}</time>${n.outgoing ? `<span class="chat-delivery ${done ? 'done' : ''}" title="${esc(status(n))}" aria-label="${esc(status(n))}">${icon(done ? 'checks' : 'check')}</span>` : ''}</footer></article>`;
  }
  function paintProgress(n) {
    const active = player && playId === n.id;
    const duration = active && Number.isFinite(player.duration) ? player.duration : n.voice.duration_ms / 1000;
    const progress = active && duration > 0 ? Math.min(1000, Math.round(player.currentTime / duration * 1000)) : 0;
    const row = host.querySelector(`[data-message="${CSS.escape(n.id)}"]`);
    if (!row) return;
    row.querySelector('[data-wave]').style.setProperty('--voice-progress', (progress / 10) + '%');
    row.querySelector('[data-voice]').value = progress;
    row.querySelectorAll('.wave-bars i').forEach((bar, i, bars) => bar.classList.toggle('played', i / bars.length < progress / 1000));
    row.querySelector('[data-elapsed]').textContent = active ? fmt(player.currentTime * 1000) + ' / ' + fmt(Math.ceil(duration) * 1000) : fmt(Math.ceil(duration) * 1000);
  }
  function update() {
    if (!host || !S) return;
    const unread = S.notes.filter(n => !n.outgoing && !n.seen);
    const badge = document.querySelector('#count-chat'); badge.textContent = unread.length || '';
    const count = id => { const n = unread.filter(n => matches(n, id)).length; return n ? ` · ${n}` : ''; };
    const options = `<option value="">${t('chat.family_hint')}${count('')}</option>`
      + S.peers.map(p => `<option value="${esc(p.id)}">${esc(p.name)}${count(p.id)}</option>`).join('');
    const selectEl = host.querySelector('#chat-peer');
    if (!S.peers.some(p => p.id === peer) && peer) {
      draft.set(peer, host.querySelector('#chat-text').value);
      peer = ''; signature = ''; host.querySelector('#chat-text').value = draft.get('') || ''; stop();
    }
    if (selectEl._options !== options) { selectEl.innerHTML = options; selectEl._options = options; }
    selectEl.value = peer;
    selectEl.disabled = !!recorder || !!pending || micPending || sending;
    renderReply();
    host.querySelector('label').textContent = t('chat.with');
    const who = S.peers.find(p => p.id === peer);
    host.querySelector('#chat-presence').textContent = who ? (who.online ? t('peer.online') : t('peer.offline')) : t('chat.family_hint');
    host.querySelector('#chat-text').placeholder = t('note.placeholder');
    host.querySelector('#chat-send').innerHTML = icon('arrowUp') + t('note.send');
    host.querySelector('#chat-send').disabled = sending || !!recorder || micPending || !S.peers.length;
    host.querySelector('#chat-mic').textContent = recorder ? t('chat.stop') : '🎙 ' + t('chat.record');
    host.querySelector('#chat-mic').disabled = sending || !!pending || micPending || !S.peers.length;
    host.querySelector('#chat-hint').textContent = t('chat.enter');
    const list = S.notes.filter(n => matches(n)).sort((a, b) => a.created_at - b.created_at);
    const sig = JSON.stringify(list.map(n => [n.id, n.text, n.to, n.voice_ready, n.seen, n.reply_to, n.cloud_ready, n.cloud_error])) + peer + L._lang;
    const history = host.querySelector('#chat-history');
    if (sig !== signature) {
      const bottom = history.scrollHeight - history.scrollTop - history.clientHeight < 60;
      history.innerHTML = list.length ? list.map(bubble).join('')
        : `<div class="chat-empty">${icon('chat', 'lg')}<b>${t('chat.empty')}</b><p>${t('chat.empty_hint')}</p></div>`;
      history.querySelectorAll('[data-voice]').forEach(el => el.oninput = () => {
        if (player && playId === el.dataset.voice) {
          const duration = Number.isFinite(player.duration) ? player.duration : S.notes.find(n => n.id === playId)?.voice?.duration_ms / 1000;
          if (duration > 0) player.currentTime = duration * Number(el.value) / 1000;
        }
      });
      if (bottom || !signature) history.scrollTop = history.scrollHeight;
      const fresh = list.find(n => !n.outgoing && !lastIds.has(n.id));
      if (shown && signature && fresh && !recorder && !player) {
        Buddy.chatReceived(fresh);
        if (!reduce) history.querySelector(`[data-message="${CSS.escape(fresh.id)}"]`)?.animate(
          [{ opacity: 0, transform: 'translateY(12px)' }, { opacity: 1, transform: 'none' }], { duration: 220 });
      }
      signature = sig;
      list.filter(n => n.voice).forEach(paintProgress);
      if (player) Buddy.chatTarget(history.querySelector(`[data-chat="play"][data-id="${CSS.escape(playId)}"]`));
    }
    lastIds = new Set(S.notes.map(n => n.id));
    const ack = list.find(n => n.outgoing && delivered.get(n.id) === false && n.to.every(p => p.delivered));
    if (ack && shown && !recorder && !player) bot.delivered(peer ? Papych.colorOf(peer) : '#60cdff');
    const cloudSent = list.find(n => n.outgoing && n.cloud_ready && cloudStates.get(n.id) === false);
    if (cloudSent && shown && !recorder && !player) bot.chatCloud();
    S.notes.forEach(n => cloudStates.set(n.id, !!n.cloud_ready));
    delivered = new Map(S.notes.filter(n => n.outgoing).map(n => [n.id, n.to.every(p => p.delivered)]));
    syncPresence(); markSeen();
  }
  function syncPresence() {
    const active = shown && tab === 'chat' && view === 'main' && !document.hidden && document.hasFocus();
    const value = peer + active;
    if (presence !== value) { presence = value; invoke('chat_presence', { peer: peer || null, active }).catch(() => {}); }
  }
  function markSeen() {
    if (!host || !shown || tab !== 'chat' || view !== 'main' || document.hidden || !Buddy.visible()) {
      clearTimeout(seenTimer); seenTimer = 0; return;
    }
    if (seenTimer) return;
    seenTimer = setTimeout(() => {
      seenTimer = 0;
      if (!shown || tab !== 'chat' || view !== 'main' || document.hidden || !Buddy.visible()) return;
      const bounds = host.querySelector('#chat-history').getBoundingClientRect();
      const ids = S.notes.filter(n => !n.outgoing && !n.seen && matches(n)).filter(n => {
        const r = host.querySelector(`[data-message="${CSS.escape(n.id)}"]`)?.getBoundingClientRect();
        return r && r.bottom > bounds.top && r.top < bounds.bottom;
      }).map(n => n.id);
      if (ids.length) invoke('notes_seen', { ids }).catch(() => {});
    }, 2000);
  }
  async function base64(blob) {
    return new Promise((resolve, reject) => { const r = new FileReader(); r.onload = () => resolve(String(r.result).split(',')[1]); r.onerror = reject; r.readAsDataURL(blob); });
  }
  async function send() {
    if (sending || recorder || micPending) return;
    const text = host.querySelector('#chat-text').value.trim();
    if (!pending && !text) return;
    sending = true; update();
    try {
      if (pending) {
        await invoke('send_voice', { data: await base64(pending.blob), mime: pending.mime,
          durationMs: pending.duration, peer: recordPeer || null, waveform: pending.waveform, replyTo });
        clearPreview();
      } else {
        await invoke('send_chat', { text, peer: peer || null, replyTo });
        draft.delete(peer); host.querySelector('#chat-text').value = '';
      }
      setReply(null);
      const r = host.querySelector('#chat-compose-origin')?.getBoundingClientRect() || host.querySelector('#chat-text').getBoundingClientRect();
      Buddy.noteSent([r.left + r.width / 2, r.top + r.height / 2], peer ? Papych.colorOf(peer) : '#60cdff');
      S = await invoke('get_state');
      host.querySelector('#chat-history').scrollTop = 1e9;
    } catch (e) { toast(esc(String(e)), 'err'); }
    finally { sending = false; update(); }
  }
  function drive(source, context) {
    analyser = context.createAnalyser(); analyser.fftSize = 256;
    source.connect(analyser);
    const values = new Uint8Array(analyser.fftSize);
    meter = setInterval(() => {
      analyser.getByteTimeDomainData(values);
      const level = Math.min(1, Math.sqrt(values.reduce((sum, v) => sum + ((v - 128) / 128) ** 2, 0) / values.length) * 5);
      Buddy.chatVoice(recorder ? 'record' : player?.paused ? 'pause' : 'play', level);
      if (recorder) recordLevels.push(Math.round(level * 255));
      host.style.setProperty('--voice-level', String(level));
    }, 90);
  }
  async function record() {
    if (pending || micPending || sending) return;
    stopPlayback(); micPending = true; const target = peer, token = ++micToken; update();
    try {
      if (!navigator.mediaDevices?.getUserMedia || !window.MediaRecorder) throw new Error(t('chat.mic_error'));
      stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      if (token !== micToken || !shown || tab !== 'chat' || view !== 'main' || peer !== target) { cleanupMic(); return; }
      const mime = ['audio/webm;codecs=opus', 'audio/ogg;codecs=opus', 'audio/mp4'].find(x => MediaRecorder.isTypeSupported(x));
      if (!mime) throw new Error(t('chat.mic_error'));
      recordPeer = peer; discard = false; chunks = []; recordLevels = [];
      recorder = new MediaRecorder(stream, { mimeType: mime, audioBitsPerSecond: 48000 });
      recorder.ondataavailable = e => { if (e.data.size) chunks.push(e.data); };
      recorder.onerror = () => { toast(t('chat.mic_error'), 'err'); cancelRecording(); };
      recorder.onstop = () => {
        const duration = Math.min(300000, Math.round(performance.now() - recordingAt));
        const blob = new Blob(chunks, { type: mime }); chunks = [];
        cleanupMic();
        if (!discard && duration >= 300 && blob.size && blob.size <= 8 * 1024 * 1024) {
          const waveform = Array.from({ length: 48 }, (_, i) => {
            const a = Math.floor(i * recordLevels.length / 48), b = Math.max(a + 1, Math.floor((i + 1) * recordLevels.length / 48));
            return Math.max(0, ...recordLevels.slice(a, b));
          });
          pending = { blob, mime: mime.split(';')[0], duration, waveform };
          previewUrl = URL.createObjectURL(blob);
          host.querySelector('#chat-record').innerHTML = `<b>${t('chat.preview')} · ${fmt(Math.ceil(duration / 1000) * 1000)}</b>
            <audio controls></audio><button class="btn sm" data-record="cancel">${t('btn.cancel')}</button>`;
          host.querySelector('#chat-record audio').src = previewUrl;
        } else if (!discard) toast(t('chat.voice_limit'), 'err');
        else clearPreview();
        update();
      };
      recordingAt = performance.now(); recorder.start(250);
      audioCtx = new AudioContext(); await audioCtx.resume();
      if (token !== micToken) return;
      drive(audioCtx.createMediaStreamSource(stream), audioCtx);
      host.querySelector('#chat-record').classList.remove('hidden');
      host.querySelector('#chat-record').innerHTML = `<span class="record-dot"></span><b>${t('chat.recording')}</b>
        <time id="chat-clock">0:00</time><div class="voice-meter"><i></i></div>
        <button class="btn sm" data-record="stop">${t('chat.stop')}</button><button class="btn sm" data-record="cancel">${t('btn.cancel')}</button>`;
      clock = setInterval(() => {
        const duration = performance.now() - recordingAt;
        host.querySelector('#chat-clock').textContent = fmt(duration);
        if (duration >= 300000 || chunks.reduce((sum, c) => sum + c.size, 0) > 8 * 1024 * 1024) finishRecording();
      }, 200);
      Buddy.chatVisit(host.querySelector('#chat-mic'), 'record');
    } catch (e) { discard = true; if (recorder?.state === 'recording') recorder.stop(); else cleanupMic(); toast(t('chat.mic_error'), 'err'); }
    finally { micPending = false; update(); }
  }
  function cleanupMic() {
    clearInterval(clock); clearInterval(meter); clock = meter = null;
    stream?.getTracks().forEach(t => t.stop()); stream = null; recorder = null;
    audioCtx?.close().catch(() => {}); audioCtx = null; analyser = null;
    Buddy.chatReturn();
  }
  function finishRecording() { if (recorder?.state === 'recording') recorder.stop(); }
  function clearPreview() {
    const audio = host?.querySelector('#chat-record audio');
    if (audio) { audio.pause(); audio.removeAttribute('src'); audio.load(); }
    if (previewUrl) URL.revokeObjectURL(previewUrl); previewUrl = null; pending = null;
    if (host) { host.querySelector('#chat-record').innerHTML = ''; host.querySelector('#chat-record').classList.add('hidden'); }
  }
  function cancelRecording() { discard = true; finishRecording(); clearPreview(); update(); }
  function stopPlayback() {
    playToken++;
    signature = '';
    player?.pause(); player = null; playId = '';
    if (playerUrl) URL.revokeObjectURL(playerUrl); playerUrl = null;
    Buddy.chatReturn();
    clearInterval(meter); meter = null;
    audioCtx?.close().catch(() => {}); audioCtx = null;

  }
  async function play(n, button) {
    if (recorder || pending || micPending) return;
    if (playId === n.id && player) {
      if (!player.paused) { player.pause(); Buddy.chatVoice('pause', 0); }
      else { await player.play(); Buddy.chatVoice('play', 0); }
      host.querySelectorAll('.voice-play').forEach(b => {
        if (b.dataset.id === n.id) { b.innerHTML = icon(player.paused ? 'play' : 'pause'); b.setAttribute('aria-label', t(player.paused ? 'chat.play' : 'chat.pause')); }
      }); return;
    }
    stopPlayback();
    const token = playToken, target = peer;
    const data = await invoke('voice_data', { id: n.id });
    if (token !== playToken || !shown || tab !== 'chat' || view !== 'main' || peer !== target) return;
    const raw = Uint8Array.from(atob(data), c => c.charCodeAt(0));
    playerUrl = URL.createObjectURL(new Blob([raw], { type: n.voice.mime }));
    player = new Audio(playerUrl); playId = n.id;
    try {
      audioCtx = new AudioContext(); await audioCtx.resume();
      if (token !== playToken) return;
      const source = audioCtx.createMediaElementSource(player); source.connect(audioCtx.destination); drive(source, audioCtx);
      player.onended = () => { stopPlayback(); signature = ''; update(); };
      player.onerror = () => { stopPlayback(); toast(t('chat.audio_error'), 'err'); signature = ''; update(); };
      player.ontimeupdate = () => paintProgress(n);
      await player.play();
      if (token !== playToken) return;
      signature = ''; update();
      Buddy.chatVisit(host.querySelector(`[data-chat="play"][data-id="${CSS.escape(n.id)}"]`) || button, 'play');
    } catch (e) { if (token === playToken) { stopPlayback(); throw new Error(t('chat.audio_error')); } }
  }
  function stop() { micToken++; clearTimeout(seenTimer); seenTimer = 0; cancelRecording(); stopPlayback(); update(); }
  return { init, update, show, compose, markSeen, stop, select, matches, reply: setReply, target: () => peer };
})();
