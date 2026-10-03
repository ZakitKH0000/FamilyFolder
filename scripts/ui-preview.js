// Просмотр вёрстки окна в браузере с примерными данными (ui/mock.js): node scripts/ui-preview.js
const http = require('http');
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..', 'crates', 'app', 'ui');
const locales = path.join(__dirname, '..', 'crates', 'core', 'locales');
// Черновики облика (не входят в программу): /design/papych-lab.html — варианты Папыча.
const design = path.join(__dirname, '..', 'design');
const types = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.png': 'image/png', '.svg': 'image/svg+xml', '.json': 'application/json' };
const port = Number(process.env.PORT || 5178);

http.createServer((req, res) => {
  const url = decodeURIComponent(req.url.split('?')[0]);
  // Строки интерфейса лежат в crates/core/locales — окно получает их от программы, здесь — файлом.
  const file = url.startsWith('/locales/') ? path.join(locales, url.slice(9))
    : url.startsWith('/design/') ? path.join(design, url.slice(8))
    : path.join(root, url === '/' ? 'index.html' : url);
  if (![root, locales, design].some(d => file.startsWith(d))) { res.writeHead(403); return res.end(); }
  fs.readFile(file, (err, data) => {
    if (err) { res.writeHead(404); return res.end('not found'); }
    res.writeHead(200, { 'Content-Type': (types[path.extname(file)] || 'application/octet-stream') + '; charset=utf-8', 'Cache-Control': 'no-store' });
    res.end(data);
  });
}).listen(port, '127.0.0.1', () => console.log(`http://localhost:${port}`));
