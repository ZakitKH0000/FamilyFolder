'use strict';
// Проверяем реальный обработчик шторки без окна и без передачи пользовательских файлов.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const source = fs.readFileSync(path.join(__dirname, '../crates/app/ui/island.js'), 'utf8');
const bootstrap = source.lastIndexOf("addEventListener('DOMContentLoaded'");
assert.ok(bootstrap > 0, 'island bootstrap found');

async function checkDrop(x, y, expectedPeers) {
  const calls = [];
  const classes = { add() {}, remove() {}, toggle() {} };
  const nodes = new Map();
  const node = name => {
    if (!nodes.has(name)) nodes.set(name, {
      classList: classes, dataset: {}, innerHTML: '',
      getBoundingClientRect: () => ({ left: 14, right: 466, top: 0, bottom: 300, width: 452 }),
    });
    return nodes.get(name);
  };
  class Papych {
    static active() {}
    static hueOf() { return 0; }
    static colorOf() { return '#60cdff'; }
    dragOver() {}
    dragEnd() {}
    async swallow() {}
    destroy() {}
  }
  const context = vm.createContext({
    window: { __TAURI__: { core: { invoke: async (command, args) => {
      calls.push({ command, args });
      return command === 'send_dropped' ? 1 : null;
    } } } },
    document: { querySelector: node }, Papych, MiniChat: {close(){}},
    setInterval() {}, setTimeout() {}, clearTimeout() {},
    icon: () => '', innerWidth: 480, innerHeight: 360, Date,
  });
  vm.runInContext(source.slice(0, bootstrap), context);
  vm.runInContext(`
    mode = 'drag';
    members = [
      { id: 'brother', name: 'Brother', p: new Papych(), el: {
        classList: { toggle() {} }, getBoundingClientRect: () => ({left:100,right:166,top:160,bottom:230})
      } },
      { id: 'mom', name: 'Mom', p: new Papych(), el: {
        classList: { toggle() {} }, getBoundingClientRect: () => ({left:300,right:366,top:160,bottom:230})
      } }
    ];
    target = 'brother'; // устаревшая подсветка: последнее движение было над братом
  `, context);
  context.dropX = x;
  context.dropY = y;
  await vm.runInContext("drop(['C:\\\\test\\\\report.txt'], dropX, dropY)", context);
  const sends = calls.filter(c => c.command === 'send_dropped');
  if (expectedPeers === null) assert.equal(sends.length, 0, 'outside island does not send');
  else {
    assert.equal(sends.length, 1, 'one drop sends once');
    assert.deepEqual(Array.from(sends[0].args.peers), expectedPeers,
      'recipient comes from release coordinates, not stale hover');
  }
}

(async () => {
  await checkDrop(320, 190, ['mom']);
  await checkDrop(240, 100, []);
  await checkDrop(470, 190, null);
  console.log('Island drop: 3 regression scenarios passed');
})().catch(error => { console.error(error); process.exitCode = 1; });
