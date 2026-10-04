// Только для просмотра шторки в обычном браузере (node scripts/ui-preview.js → /island.html?peek).
// Дополняет mock.js: события программы, курсор и перетаскивание файлов. В программе не работает.
// ?manual — шторка спрятана, пока её не откроют снаружи (снимки для GitHub: window.__emit, window.__drag).
(function () {
  if (!window.__MOCK__) return;
  const q = new URLSearchParams(location.search);
  const T = window.__TAURI__, base = T.core.invoke;
  const listeners = {};
  const manual = q.has('manual');
  let drag = null;
  const reason = ['peek', 'drag', 'offer', 'received', 'delivered', 'joined'].find(r => q.has(r)) || 'peek';
  const en = q.get('lang') === 'en';
  const DATA = {
    offer: { id: '2', from: en ? 'Brother' : 'Брат', title: en ? 'report.docx' : 'отчёт.docx', detail: en ? '245 KB' : '245 КБ', is_update: false, has_exe: false },
    received: { title: en ? 'summer_house.jpg' : 'фото_дачи.jpg', path: '', conflicts: 0 },
    delivered: { title: en ? 'recipes.txt' : 'рецепты.txt', to: en ? 'Brother' : 'Брат' },
    joined: { id: 'm', name: en ? "Mom's laptop" : 'Ноутбук мамы',
      added_by: en ? 'Brother' : 'Брат', files: q.has('share') ? 12 : 0 },
  };
  const emit = (name, payload) => (listeners[name] || []).forEach(f => f({ payload }));
  window.__emit = emit;
  window.__drag = p => drag?.({ payload: p });
  T.event.listen = async (name, fn) => { (listeners[name] ||= []).push(fn); return () => {}; };
  T.webview = { getCurrentWebview: () => ({ onDragDropEvent: fn => { drag = fn; } }) };
  T.core.invoke = async (cmd, args) => {
    if (cmd === 'island_ready') return manual ? [] : [{ reason, data: DATA[reason] ?? null }];
    // Спряталась — через секунду показать снова, чтобы было что смотреть.
    if (cmd === 'island_close') { if (!manual) setTimeout(() => emit('island-open', { reason, data: DATA[reason] ?? null }), 1200); return null; }
    if (cmd.startsWith('island_') || cmd === 'show_main') return null;
    return base(cmd, args);
  };
  const k = () => devicePixelRatio || 1;
  addEventListener('pointermove', e => emit('island-cursor', [e.clientX, e.clientY, (e.buttons & 1) === 1]));
  addEventListener('dragover', e => {
    e.preventDefault();
    emit('island-cursor', [e.clientX, e.clientY, true]);
    drag?.({ payload: { type: 'over', position: { x: e.clientX * k(), y: e.clientY * k() } } });
  });
  addEventListener('drop', e => {
    e.preventDefault();
    const paths = [...e.dataTransfer.files].map(f => f.name);
    drag?.({ payload: { type: 'drop', paths: paths.length ? paths : ['фото.jpg'], position: { x: e.clientX * k(), y: e.clientY * k() } } });
  });
  if (!manual) document.documentElement.style.background = 'linear-gradient(160deg, #6d8fb5, #b9c7d8)';
})();
