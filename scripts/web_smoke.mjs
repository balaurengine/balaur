#!/usr/bin/env node
// Boot every pack of a web bundle in headless Chrome and fail on anything the
// page logs above INFO: a WebGPU validation message, a script ERROR or WARN,
// an exception, a failed fetch. Native naga accepts WGSL a browser refuses, so
// only a browser finds those. Also fails a canvas that drew nothing.
//
// Usage: web_smoke.mjs <play-dir>   (balaur.js, balaur_bg.wasm, *.bpak)
//   CHROME   the browser binary; found on PATH or in /Applications otherwise
//   SMOKE_SECONDS  how long each pack runs, default 8
import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';

const dir = path.resolve(process.argv[2] ?? 'dist/play');
const seconds = Number(process.env.SMOKE_SECONDS ?? 8);
const fail = (message) => {
  console.log(`::error::${message}`);
  process.exit(1);
};

const packs = fs.readdirSync(dir).filter((f) => f.endsWith('.bpak')).map((f) => f.slice(0, -5));
if (!packs.includes('editor') || packs.length < 2) fail(`${dir} holds no editor and examples to boot`);
// Each game on its own, then the editor over one of them, the way the site opens both.
const cases = packs.filter((p) => p !== 'editor').map((p) => ({ name: p, start: `start('c', '/${p}.bpak')` }));
cases.push({ name: 'editor', start: `start_editor('c', '/editor.bpak', '/hello.bpak')` });

const page = (start) => `<!doctype html><html><head><link rel="icon" href="data:,"></head>
<body style="margin:0"><canvas id="c" style="display:block;width:960px;height:540px"></canvas>
<script type="module">
if (!navigator.gpu || !(await navigator.gpu.requestAdapter())) console.error('smoke: no WebGPU adapter');
const mod = await import('/balaur.js');
await mod.default({ module_or_path: '/balaur_bg.wasm' });
mod.${start}.catch((e) => console.error('smoke: start failed', e));
</script></body></html>`;

// Somewhere to bring the GPU process up before any pack is judged. Chrome
// answers the first requestAdapter with null and logs a stale instance while
// SwiftShader settles, and that used to fail whichever pack happened to be
// first -- angrynerds, which drew correctly the whole time.
const warmup = `<!doctype html><html><body><script type="module">
for (let i = 0; i < 40; i++) {
  const a = navigator.gpu ? await navigator.gpu.requestAdapter().catch(() => null) : null;
  if (a) break;
  await new Promise((r) => setTimeout(r, 250));
}
document.title = 'warm';
</script></body></html>`;

let current = '';
const types = { '.js': 'text/javascript', '.wasm': 'application/wasm' };
const server = http.createServer((q, s) => {
  const url = new URL(q.url, 'http://x');
  if (url.pathname === '/') {
    s.setHeader('Content-Type', 'text/html');
    s.end(current ? page(current) : warmup);
    return;
  }
  // Resolved under `dir`, not flattened to its basename: balaur.js imports
  // wasm-bindgen's `snippets/<crate>-<hash>/inline0.js`, and a basename turns
  // that into a 404 the page reports as "failed to fetch balaur.js".
  const root = path.resolve(dir);
  const file = path.resolve(root, '.' + path.posix.normalize(url.pathname));
  const inside = file === root || file.startsWith(root + path.sep);
  if (!inside || !fs.existsSync(file) || !fs.statSync(file).isFile()) {
    s.statusCode = 404;
    s.end();
    return;
  }
  s.setHeader('Content-Type', types[path.extname(file)] ?? 'application/octet-stream');
  s.end(fs.readFileSync(file));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const origin = `http://127.0.0.1:${server.address().port}`;

const chrome = process.env.CHROME ?? findChrome();
// SwiftShader is WebGPU on a runner with no GPU; Chrome still validates every
// shader the way it would on real hardware.
const flags = ['--enable-unsafe-webgpu'];
if (os.platform() === 'linux') {
  flags.push('--enable-features=Vulkan', '--use-angle=vulkan', '--use-vulkan=swiftshader',
    '--use-webgpu-adapter=swiftshader', '--disable-vulkan-surface', '--no-sandbox');
}
const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'balaur-smoke-'));
const port = 9300 + Math.floor(Math.random() * 600);
const browser = spawn(chrome, ['--headless=new', `--remote-debugging-port=${port}`,
  `--user-data-dir=${profile}`, '--no-first-run', '--no-default-browser-check',
  '--window-size=1280,800', ...flags, 'about:blank'],
  { stdio: ['ignore', 'ignore', 'pipe'] });
// Chrome's own reason for not starting. `stdio: 'ignore'` threw it away, so a
// browser that refused to come up read only as "did not open a debugging
// port" -- true, and not something anyone can act on.
let said = '';
browser.stderr.on('data', (d) => { said = (said + d).slice(-4096); });
let exited = null;
browser.on('exit', (code, signal) => { exited = signal ?? code; });
// However this ends, a failure, a closed pipe or ^C, the browser goes with it.
process.on('exit', () => browser.kill('SIGKILL'));
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(signal, () => process.exit(1));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let target;
// `exited` stops the wait the moment the browser is gone: a Chrome that dies
// on a bad flag did so in the first second, and 20s of polling a dead port
// only delays the message.
for (let i = 0; i < 100 && !target && exited === null; i++) {
  await sleep(200);
  try {
    const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
    target = list.find((t) => t.type === 'page');
  } catch {}
}
if (!target) {
  const how = exited === null ? 'is still running' : `exited ${exited}`;
  const why = said.trim().split('\n').slice(-6).join(' | ');
  fail(`${chrome} did not open a debugging port; it ${how}${why ? `: ${why}` : ' and said nothing'}`);
}
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener('open', r));
let id = 0;
const pending = new Map();
const send = (method, params = {}) => new Promise((r) => {
  pending.set(++id, r);
  ws.send(JSON.stringify({ id, method, params }));
});
let heard = [];
ws.addEventListener('message', (e) => {
  const m = JSON.parse(e.data);
  if (m.id) {
    pending.get(m.id)?.(m.result ?? m.error);
    pending.delete(m.id);
  } else if (m.method === 'Runtime.consoleAPICalled') {
    const text = m.params.args.map((a) => a.value ?? a.description).join(' ');
    // The engine logs every level through console.log, as ` WARN target: …`.
    if (m.params.type !== 'log' && m.params.type !== 'info' && m.params.type !== 'debug') heard.push(text);
    else if (/\b(WARN|ERROR)\b/.test(text)) heard.push(text);
  } else if (m.method === 'Runtime.exceptionThrown') {
    const d = m.params.exceptionDetails;
    heard.push(d.exception?.description ?? d.text);
  } else if (m.method === 'Log.entryAdded' && m.params.entry.level !== 'verbose' && m.params.entry.level !== 'info') {
    heard.push(m.params.entry.text + (m.params.entry.url ? ` (${m.params.entry.url})` : ''));
  }
});
await send('Runtime.enable');
await send('Log.enable');
await send('Page.enable');

// Distinct colours in a grid of samples: one colour is a frame that never drew.
// Through `toDataURL`: a WebGPU canvas hands `drawImage` nothing between frames.
const colours = `(async () => {
  const shot = new Image();
  shot.src = document.getElementById('c').toDataURL('image/png');
  await shot.decode();
  const flat = document.createElement('canvas');
  flat.width = 64; flat.height = 36;
  const g = flat.getContext('2d');
  g.drawImage(shot, 0, 0, 64, 36);
  const px = g.getImageData(0, 0, 64, 36).data;
  const seen = new Set();
  for (let i = 0; i < px.length; i += 4) seen.add(px[i] << 16 | px[i + 1] << 8 | px[i + 2]);
  return seen.size;
})()`;

const failures = [];
current = '';
await send('Page.navigate', { url: `${origin}/` });
for (let i = 0; i < 40; i++) {
  const warm = (await send('Runtime.evaluate', { expression: 'document.title', returnByValue: true }))
    ?.result?.value;
  if (warm === 'warm') break;
  await sleep(500);
}
// Whatever the browser said while it was starting is not a pack's fault.
heard = [];

for (const c of cases) {
  current = c.start;
  heard = [];
  await send('Page.navigate', { url: `${origin}/` });
  await sleep(seconds * 1000);
  const drawn = (await send('Runtime.evaluate', { expression: colours, awaitPromise: true, returnByValue: true }))
    ?.result?.value ?? 0;
  const problems = [...new Set(heard)];
  if (drawn < 2) problems.push(`the canvas is one flat colour after ${seconds} s: nothing drew`);
  console.log(`  ${c.name.padEnd(12)} ${problems.length ? 'FAILED' : 'ok'}`);
  for (const p of problems.slice(0, 8)) console.log(`      ${p.split('\n').slice(0, 6).join('\n      ')}`);
  if (problems.length) failures.push(c.name);
}
ws.close();
const closed = new Promise((r) => browser.on('exit', r));
browser.kill();
await closed;
// A browser killed mid-download leaves its socket open, which keeps node alive.
server.closeAllConnections();
server.close();
fs.rmSync(profile, { recursive: true, force: true, maxRetries: 5 });
if (failures.length) fail(`in a browser: ${failures.join(', ')}`);

function findChrome() {
  for (const name of ['google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser']) {
    try {
      return execFileSync('which', [name]).toString().trim();
    } catch {}
  }
  const mac = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
  if (fs.existsSync(mac)) return mac;
  fail('no Chrome to boot the bundle in: install one or set CHROME');
}
