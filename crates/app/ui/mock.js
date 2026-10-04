// Только для просмотра вёрстки в обычном браузере (в программе не загружается).
// ?onb — знакомство, ?empty — пусто, ?cloud — облако подключено, ?lang=en — язык (и примеры по-английски,
// для снимков на GitHub), ?paused — пауза, ?update — готово обновление.
// Шторка: island.html?peek (?drag, ?offer, ?received, ?delivered, ?joined).
(function () {
  if (window.__TAURI__) return;
  const q = new URLSearchParams(location.search);
  const now = Date.now();
  const GB = 1024 ** 3, MB = 1024 ** 2;
  const empty = q.has('empty');
  const en = q.get('lang') === 'en';
  const L = (ru, eng) => (en ? eng : ru);
  const BRO = L('Брат', 'Brother'), MOM = L('Мама', 'Mom'), DIRECT = L('напрямую', 'direct');
  const ME = L('Компьютер Закира', "Zakir's PC"), FOLDER = L('C:\\Users\\Zakir\\Общая', 'C:\\Users\\Zakir\\Family Folder');
  const item = (o) => ({ peer_id: 'b', batch: o.id, kind: 'file', is_folder: false, is_update: false, files: 1, done: 0, speed: 0, message: null, source: null, conflicts: [], path: 'x', cloud: 'none', cloud_done: 0, has_exe: false, auto: false, created_at: now - 60e3, updated_at: now, ...o });
  const state = {
    me: '7f3a9c0e5b2d4f61a8e9c0d1b2a3f4e5', my_name: ME, folder: FOLDER,
    onboarded: !q.has('onb'),
    peers: empty ? [] : [
      { id: 'b', name: BRO, online: true, route: DIRECT, last_seen: now, version: L('1.1.0', '1.4.1'), outdated: false },
      { id: 'm', name: MOM, online: false, route: '', last_seen: now - 26 * 3600e3, version: L('1.0.1', '1.4.1'), outdated: !en },
    ],
    incoming: empty ? [] : [
      item({ id: '1', peer: BRO, item: L('Фото с дачи', 'Summer photos'), kind: 'folder', is_folder: true, files: 128, total: 2.3 * GB, state: 'new', created_at: now - 60e3 }),
      item({ id: '2', peer: BRO, item: L('отчёт.docx', 'report.docx'), kind: 'doc', is_update: true, total: 245 * 1024, state: 'new', created_at: now - 4 * 60e3 }),
      item({ id: '3', peer: BRO, item: L('видео_свадьба.mp4', 'wedding_video.mp4'), kind: 'video', total: 4.2 * GB, done: 1.9 * GB, speed: 14.6 * MB, state: 'downloading', source: DIRECT, created_at: now - 20 * 60e3 }),
      item({ id: '4', peer: MOM, peer_id: 'm', item: 'setup_game.exe', kind: 'app', total: 880 * MB, state: 'new', has_exe: true, created_at: now - 30 * 60e3 }),
      item({ id: '5', peer: BRO, item: L('музыка.zip', 'music.zip'), kind: 'archive', total: 312 * MB, done: 312 * MB, state: 'done', source: DIRECT, conflicts: [L('заметки (версия от Брат).txt', 'notes (version from Brother).txt')], created_at: now - 26 * 3600e3, updated_at: now - 25 * 3600e3 }),
      item({ id: '6', peer: BRO, item: 'IMG_2041.jpg', kind: 'image', total: 3.2 * MB, done: 3.2 * MB, state: 'done', auto: true, created_at: now - 3 * 86400e3, updated_at: now - 3 * 86400e3 }),
    ],
    outgoing: empty ? [] : [
      item({ id: 'o1', peer: BRO, item: L('игра.zip', 'game.zip'), kind: 'archive', total: 12.4 * GB, done: 7.9 * GB, speed: 11.2 * MB, state: 'downloading', batch: 'g1', created_at: now - 50 * 60e3 }),
      item({ id: 'o1m', peer: MOM, peer_id: 'm', item: L('игра.zip', 'game.zip'), kind: 'archive', total: 12.4 * GB, state: 'pending', batch: 'g1', created_at: now - 50 * 60e3 }),
      item({ id: 'o2', peer: BRO, item: L('Документы', 'Documents'), kind: 'folder', is_folder: true, files: 14, total: 48 * MB, speed: 3.1 * MB, state: 'offered', cloud: 'uploading', cloud_done: 20 * MB, created_at: now - 5 * 60e3 }),
      item({ id: 'o3', peer: BRO, item: L('фото_паспорта.pdf', 'passport_scan.pdf'), kind: 'doc', total: 2.1 * MB, state: 'offered', cloud: 'uploaded', created_at: now - 2 * 3600e3 }),
      item({ id: 'o4', peer: BRO, item: L('рецепты.txt', 'recipes.txt'), kind: 'doc', is_update: true, total: 12 * 1024, done: 12 * 1024, state: 'delivered', batch: 'g4', created_at: now - 3 * 3600e3, updated_at: now - 3 * 3600e3 }),
      item({ id: 'o4m', peer: MOM, peer_id: 'm', item: L('рецепты.txt', 'recipes.txt'), kind: 'doc', is_update: true, total: 12 * 1024, state: 'declined', batch: 'g4', created_at: now - 3 * 3600e3, updated_at: now - 3 * 3600e3 }),
    ],
    preparing: empty ? [] : [{ item: L('Отпуск 2026', 'Vacation 2026'), done: 1.2 * GB, total: 3.4 * GB }],
    settings: { device_name: ME, folder: FOLDER, onboarded: true, language: q.get('lang') || '', auto_accept: 'Photos', auto_accept_mb: 1024, auto_accept_exe: false, speed_limit_kbps: 0, cloud_mode: 'WhenNeeded', dock_panel: true, autostart: true, notify_delivered: true, island: true, notify_via: 'Island', tour_seen: 1, tips: true },
    cloud: q.has('cloud')
      ? { connected: true, provider: 'webdav', local: false, url: 'https://app.koofr.net/dav/Koofr', user: 'zakir', login: 'zakir · app.koofr.net', uploading: true, auth_code: null, auth_url: null, error: null, client_id: '', client_secret: '' }
      : { connected: false, provider: '', local: false, url: '', user: '', login: '', uploading: false, auth_code: q.has('auth') ? '4821-7730' : null, auth_url: 'https://ya.ru/device', error: null, client_id: '', client_secret: '' },
    version: '1.4.1',
    history_shares: q.has('share') ? [{ peer_id: 'm', name: MOM, added_by: BRO, files: 12 }] : [],
    lang: q.get('lang') || 'ru',
    paused_until: q.has('paused') ? now + 3600e3 : 0,
    update: q.has('update') ? '1.2.0' : null,
    notes: empty ? [] : [
      { id: 'n1', outgoing: false, peer_id: 'b', peer: BRO, text: L('Фото с выходных на даче 📸\nhttps://photos.example.com/dacha','Photos from the weekend 📸\nhttps://photos.example.com/weekend'), created_at: now - 5 * 60e3, seen: false, to: [] },
      { id: 'n2', outgoing: true, peer_id: '', peer: '', text: L('Купите хлеба по дороге', 'Grab some bread on the way home 🍞'), created_at: now - 3600e3, seen: true, to: [{ id: 'b', name: BRO, delivered: true, needs_update: false }, { id: 'm', name: MOM, delivered: false, needs_update: !en }] },
    ],
  };
  const LANGS = [['ru', 'Русский'], ['en', 'English'], ['uk', 'Українська'], ['de', 'Deutsch'], ['es', 'Español'], ['fr', 'Français'], ['pt', 'Português'], ['tr', 'Türkçe'], ['zh', '中文']];
  async function strings() {
    const lang = state.settings.language || 'ru';
    const load = async l => { try { return await (await fetch(`/locales/${l}.json`)).json(); } catch { return {}; } };
    return { ...(await load('ru')), ...(await load('en')), ...(await load(lang)), _lang: lang, _langs: LANGS };
  }
  // События программы: страница подписывается, снимки для GitHub (scripts/showcase.js) вызывают __emit.
  const listeners = {};
  // Экскурсия: где «окно программы» (?main=x,y,w,h) — в программе это передаёт tour.rs.
  if (q.has('main')) window.TOUR = { main: q.get('main').split(',').map(Number) };
  window.__emit = (name, payload) => (listeners[name] || []).forEach(f => f({ payload }));
  window.__TAURI__ = {
    core: {
      invoke: async (cmd, args) => {
        console.log('invoke', cmd, args);
        if (cmd === 'get_state') return state;
        if (cmd === 'get_strings') return strings();
        if (cmd === 'save_settings') { state.settings = args.settings; return null; }
        if (cmd === 'pause') { state.paused_until = args.minutes ? Date.now() + args.minutes * 60e3 : 8.64e15; return null; }
        if (cmd === 'resume') { state.paused_until = 0; return null; }
        if (cmd === 'cloud_folders') return [['OneDrive', 'C:\\Users\\Zakir\\OneDrive\\Family Folder sync'], ['Google Drive', 'G:\\Мой диск\\Family Folder sync']];
        if (cmd === 'create_invite') return 'MNXG-4ZLB-OJSW-Q2LT-MFZG-K3TB-NFXG-IZLB-OJSW-Q2LT-MFZG-K3TB-NFXG-IZLB-OJSW-Q2LT-MFZG-K3TB-NFXG-IZLB';
        if (cmd === 'join') return BRO;
        if (cmd === 'send_dropped') return args.paths.length;
        if (cmd === 'share_history') {
          state.history_shares = state.history_shares.filter(r => r.peer_id !== args.peer);
          window.__emit('state', state);
          return null;
        }
        if (cmd === 'send_note') {
          state.notes.unshift({ id: 'n' + Date.now(), outgoing: true, peer_id: '', peer: '', text: args.text, created_at: Date.now(), seen: true, to: state.peers.filter(p => !args.peers.length || args.peers.includes(p.id)).map(p => ({ id: p.id, name: p.name, delivered: false, needs_update: false })) });
          return null;
        }
        return null;
      },
    },
    event: { listen: async (name, fn) => { (listeners[name] ||= []).push(fn); return () => {}; } },
  };
  window.__MOCK__ = true;
})();
