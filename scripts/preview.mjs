import http from 'node:http';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

// Serve only public website assets, never the adjacent FSX installation.
const files = new Map([
  ['/', ['index.html', 'text/html; charset=utf-8']],
  ['/index.html', ['index.html', 'text/html; charset=utf-8']],
  ['/styles.css', ['styles.css', 'text/css; charset=utf-8']],
  ['/app.js', ['app.js', 'text/javascript; charset=utf-8']],
  ['/docs.js', ['docs.js', 'text/javascript; charset=utf-8']],
  ['/favicon.svg', ['favicon.svg', 'image/svg+xml']],
  ...['c172-viewer.png', 'c172-flight.png', 'c172-ground.png'].map(name => [`/assets/screenshots/${name}`, [`assets/screenshots/${name}`, 'image/png']]),
  ...['openfsx-light.svg', 'openfsx-dark.svg', 'openfsx-mono.svg', 'openfsx-icon.svg'].map(name => [`/assets/brand/${name}`, [`assets/brand/${name}`, 'image/svg+xml']]),
  ...['openfsx-avatar.png', 'openfsx-preview.png'].map(name => [`/assets/brand/${name}`, [`assets/brand/${name}`, 'image/png']]),
]);
http.createServer(async (req, res) => {
  try {
    const file = files.get(new URL(req.url, 'http://localhost').pathname);
    if (!file) { res.writeHead(404).end('Not found'); return; }
    const data = await readFile(fileURLToPath(new URL(`../site/${file[0]}`, import.meta.url)));
    res.writeHead(200, { 'Content-Type': file[1], 'Cache-Control': 'no-store' });
    res.end(data);
  } catch {
    res.writeHead(500).end('Preview unavailable');
  }
}).listen(4174, '127.0.0.1', () => console.log('Local: http://127.0.0.1:4174'));
