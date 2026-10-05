'use strict';
// Тот же Chat, что в основном окне; здесь только поведение шторки и её Папыч.
let view = 'main', tab = 'chat', confirmPending = null;
function closeViews() {}
function switchTab() { MiniChat.open(); }
function linkify(text) {
  return esc(text).replace(/https?:\/\/[^\s<>]+/g, url => `<a href="#" data-mini-url="${url}">${url}</a>`);
}
function toast(message) {
  const el = $('#toast'); el.innerHTML = message; el.classList.remove('hidden');
  setTimeout(() => el.classList.add('hidden'), 4500);
}
function confirmDialog(title, text, ok) {
  return new Promise(resolve => {
    confirmPending = resolve;
    const el = $('#mini-confirm');
    el.innerHTML = `<b>${esc(title)}</b><p>${esc(text)}</p><button data-ok>${esc(ok)}</button><button>${t('btn.cancel')}</button>`;
    el.classList.remove('hidden');
    el.onclick = e => { const b = e.target.closest('button'); if (!b) return; el.classList.add('hidden'); confirmPending=null; resolve(b.hasAttribute('data-ok')); };
  });
}
const Buddy = (() => {
  const reduce = matchMedia('(prefers-reduced-motion:reduce)').matches;
  let flight = null, token = 0, held = null, voiceMode = null;
  const box = $('#bot');
  function chatVoice(mode, level = 0) {
    voiceMode = mode;
    if (!flight || flight.playState === 'finished') bot.chatVoice(mode, level);
  }
  function chatTarget(button) {
    if (!voiceMode || !button) return;
    const pressed = held?.classList.contains('pressed'); held = button;
    if (pressed || flight?.playState === 'finished') button.classList.add('pressed');
  }
  function chatVisit(button, mode) {
    if (!button) return;
    const seq = ++token, matrix = new DOMMatrix(getComputedStyle(box).transform);
    flight?.cancel(); flight = null; held?.classList.remove('pressed'); held = button;
    chatVoice(mode);
    if (reduce) { held.classList.add('pressed'); return; }
    const a = box.getBoundingClientRect(), b = button.getBoundingClientRect();
    const x = b.left + b.width/2 - a.left - a.width/2, y = b.top - a.bottom + 12;
    bot.leap(true); box.classList.add('mini-visiting');
    flight = box.animate([{transform:`translate(${matrix.m41}px,${matrix.m42}px)`},
      {transform:`translate(${x/2}px,${y/2-55}px) scale(.85)`}, {transform:`translate(${x}px,${y}px)`}],
      {duration:450,easing:'ease-in-out',fill:'forwards'});
    flight.finished.then(() => { if (seq !== token) return; bot.leap(false); bot.chatVoice(voiceMode || mode); held?.classList.add('pressed'); }).catch(() => {});
  }
  function chatReturn() {
    if (!voiceMode && !flight) return;
    const seq = ++token, transform = getComputedStyle(box).transform;
    flight?.cancel(); flight = null; voiceMode = null; held?.classList.remove('pressed'); held = null;
    bot.settle();
    if (reduce || transform === 'none') { box.classList.remove('mini-visiting'); return; }
    flight = box.animate([{transform},{transform:'translate(0,0)'}], {duration:300,fill:'forwards',easing:'ease-out'});
    flight.finished.then(() => { if(seq !== token) return; flight.cancel(); flight=null; box.classList.remove('mini-visiting'); }).catch(() => {});
  }
  return {actor:()=>bot, visible:()=>mode==='chat' && !document.hidden,
    watch:()=>{}, chatVoice, chatVisit, chatReturn, chatTarget,
    chatReceived:()=>{ if (!voiceMode) bot.receive({note:true}); },
    noteSent:(from,color)=>bot.swallow({name:t('chat.title')},{from,launch:color})};
})();
const MiniChat = (() => {
  let initialized = false;
  function init() {
    if (initialized) return; initialized=true; Chat.init({embedded:true});
    document.addEventListener('keydown', e => { if(e.key==='Escape' && mode==='chat') close(); });
    $('#chat').addEventListener('click', e => {
      const a=e.target.closest('[data-mini-url]'); if(a){e.preventDefault(); invoke('open_url',{url:a.textContent}).catch(e=>toast(esc(e)));}
    });
  }
  async function openChat(id) {
    init();
    S=await invoke('get_state');
    const n=S.notes.find(n=>n.id===id);
    demo=false; mode='chat'; isl.dataset.mode='chat'; isl.classList.remove('drag'); isl.classList.add('in');
    $('#offers').innerHTML=''; $('#family').innerHTML=''; $('#chips').innerHTML=''; $('#track').classList.remove('on');
    $('#chat').classList.remove('hidden'); Papych.active(true); until=0;
    head(t('chat.title'),t('chat.mini_hint'));
    acts([[icon('app'),()=>{invoke('show_chat',{peer:Chat.target()});close();},'icon',t('island.open_window')],
      [icon('close'),()=>close(),'icon',t('btn.close')]]);
    Chat.show(true); Chat.select(n ? n.group ? '' : n.peer_id : Chat.target());
    if(n) Chat.reply(n.id);
    await invoke('island_chat_focus',{active:true}); Chat.compose();
    if(!matchMedia('(prefers-reduced-motion:reduce)').matches) $('#chat').animate([{opacity:0,transform:'translateY(-10px)'},{opacity:1,transform:'none'}],{duration:220});
    bot.chatUnfold();
  }
  function stop() { if (!initialized) return; confirmPending?.(false); confirmPending=null; $('#mini-confirm').classList.add('hidden'); Chat.show(false); $('#chat').classList.add('hidden'); invoke('island_chat_focus',{active:false}); }
  function update() { if(initialized) Chat.update(); }
  return {init,open:openChat,close:stop,update};
})();
window.MiniChat = MiniChat;
