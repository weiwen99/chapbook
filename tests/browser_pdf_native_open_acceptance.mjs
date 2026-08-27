// PDF native-open 验收 — 确定性 CDP 浏览器验收: .pdf 文件的预览/树导航/迷你
// 列表点击一律 window.open 新 tab (浏览器原生查看器), 不 SPA/不进预览面板;
// 目录名 `folder.pdf` 不受影响; 服务端对 PDF 200 响应省略 sandbox 并强制
// nosniff, download 仍 sandbox + attachment。
//
// 手动调用 (非 cargo test), 自包含: 仅 Node 内置模块 + 系统 Chrome 的 CDP
// WebSocket (无任何 npm 包); 自建临时 fixture, 自行启动 target/release/chapbook
// 并监听固定测试端口, finally 清理 server/chrome/fixture; 任一断言失败以
// 非零退出.
//
// 运行:
//   cargo build --release && node tests/browser_pdf_native_open_acceptance.mjs
//
// Chrome 定位: $CHROME_BIN 优先, 其次 ms-playwright / Chrome-for-Testing 已知
// 路径 (macOS 与 Linux) 与系统常见位置.
//
// 场景:
//   1  目录模式行 ◈ 预览 PDF: spy 收到完整 Full URL, 面板不打开, URL 不变
//   2  树模式点击 PDF 节点: spy 收到 URL, pane 保持先前内容, URL 不变
//   3  树模式点击 folder.pdf 目录: 正常展开, spy 零调用
//   4  树模式面板迷你列表点击 PDF 文件: spy 收到 URL, 无 SPA 导航;
//      迷你列表点击 folder.pdf 目录: SPA 下钻, spy 零调用
//   5  目录模式 ◈ 预览 folder.pdf 目录: 面板显示目录 fragment, spy 零调用
//   6  目录模式选中 PDF 行按 p: spy 收到 URL, 面板不打开
//   7  服务端头: /doc.pdf 200 无 CSP + nosniff + application/pdf;
//      ?download=1 sandbox + attachment
// 全部场景零页面错误.

import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import net from 'node:net';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const results = [];
function rec(name, pass, facts) {
  results.push({ name, pass });
  console.log((pass ? 'PASS' : 'FAIL') + ' | ' + name + ' | ' + facts);
}

/* ---------- Chrome 定位 (CHROME_BIN 优先, 其次已知缓存/系统路径) ---------- */

function findChrome() {
  const env = process.env.CHROME_BIN;
  if (env && existsSync(env)) return env;
  const caches = [];
  if (process.platform === 'darwin') {
    caches.push(join(process.env.HOME || '', 'Library', 'Caches', 'ms-playwright'));
  }
  caches.push(join(process.env.HOME || '', '.cache', 'ms-playwright'));
  caches.push(join(process.env.HOME || '', '.cache', 'chrome-for-testing'));
  const found = [];
  for (const cache of caches) {
    if (!existsSync(cache)) continue;
    for (const dir of readdirSync(cache)) {
      const base = join(cache, dir);
      if (!dir.startsWith('chromium') && !dir.startsWith('chrome')) continue;
      const isShell = dir.startsWith('chromium_headless_shell') || dir.startsWith('headless_shell');
      const full = [
        join(base, 'chrome-mac-arm64', 'Google Chrome for Testing.app', 'Contents', 'MacOS', 'Google Chrome for Testing'),
        join(base, 'chrome-mac', 'Google Chrome for Testing.app', 'Contents', 'MacOS', 'Google Chrome for Testing'),
        join(base, 'chrome-mac-arm64', 'Chromium.app', 'Contents', 'MacOS', 'Chromium'),
        join(base, 'chrome-mac', 'Chromium.app', 'Contents', 'MacOS', 'Chromium'),
        join(base, 'chrome-linux64', 'chrome'),
        join(base, 'chrome-linux', 'chrome'),
        join(base, 'chrome-mac', 'Google Chrome for Testing'),
        join(base, 'chrome-win64', 'chrome.exe'),
      ];
      const shell = [
        join(base, 'chrome-mac-arm64', 'headless_shell'),
        join(base, 'chrome-mac', 'headless_shell'),
        join(base, 'chrome-linux64', 'headless_shell'),
        join(base, 'chrome-linux', 'headless_shell'),
        join(base, 'headless_shell'),
      ];
      const ver = parseInt((dir.match(/(\d+)\s*$/) || [])[1] || '0', 10);
      for (const c of (isShell ? shell : full)) {
        if (existsSync(c) && !found.some((f) => f.path === c)) {
          found.push({ path: c, isShell, ver });
        }
      }
    }
  }
  found.sort((a, b) => (a.isShell === b.isShell ? b.ver - a.ver : a.isShell ? 1 : -1));
  const picked = found.map((f) => f.path);
  const system = [
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Chromium.app/Contents/MacOS/Chromium',
    '/usr/bin/google-chrome',
    '/usr/bin/google-chrome-stable',
    '/usr/bin/chromium',
    '/usr/bin/chromium-browser',
    '/opt/google/chrome/chrome',
  ];
  for (const c of system) {
    if (existsSync(c) && !picked.includes(c)) picked.push(c);
  }
  return picked.length > 0 ? picked[0] : null;
}

/* ---------- CDP 客户端 (浏览器级 WebSocket, flatten session) ---------- */

class Cdp {
  constructor(wsUrl) {
    this.wsUrl = wsUrl;
    this.ws = new WebSocket(wsUrl);
    this.nextId = 1;
    this.pending = new Map();
    this.handlers = new Map();
  }

  async open() {
    await new Promise((resolve, reject) => {
      this.ws.addEventListener('open', () => resolve(), { once: true });
      this.ws.addEventListener('error', () => reject(new Error('CDP websocket connect failed: ' + this.wsUrl)), { once: true });
    });
    this.ws.addEventListener('message', (ev) => this._onMessage(ev.data));
  }

  _onMessage(data) {
    let msg;
    try {
      msg = JSON.parse(data);
    } catch {
      return;
    }
    if (msg.id !== undefined) {
      const p = this.pending.get(msg.id);
      if (!p) return;
      this.pending.delete(msg.id);
      clearTimeout(p.timer);
      if (msg.error) p.reject(new Error(msg.error.message));
      else p.resolve(msg.result);
      return;
    }
    if (msg.method) {
      const hs = this.handlers.get(msg.method);
      if (hs) {
        for (const h of [...hs]) h(msg.params, msg.sessionId);
      }
    }
  }

  send(method, params = {}, sessionId) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error('CDP timeout: ' + method));
      }, 20000);
      this.pending.set(id, { resolve, reject, timer });
      const msg = { id, method, params };
      if (sessionId) msg.sessionId = sessionId;
      this.ws.send(JSON.stringify(msg));
    });
  }

  on(method, handler) {
    if (!this.handlers.has(method)) this.handlers.set(method, []);
    this.handlers.get(method).push(handler);
  }

  remove(method, handler) {
    const hs = this.handlers.get(method);
    if (!hs) return;
    const i = hs.indexOf(handler);
    if (i >= 0) hs.splice(i, 1);
  }

  close() {
    try {
      this.ws.close();
    } catch {
      /* 已关闭 */
    }
  }
}

/* ---------- 页面封装 ---------- */

class Page {
  constructor(cdp, targetId, sessionId) {
    this.cdp = cdp;
    this.targetId = targetId;
    this.sessionId = sessionId;
    this._listeners = [];
  }

  send(method, params) {
    return this.cdp.send(method, params, this.sessionId);
  }

  on(method, handler) {
    this.cdp.on(method, handler);
    this._listeners.push([method, handler]);
  }

  async evaluate(expression) {
    const r = await this.send('Runtime.evaluate', {
      expression,
      returnByValue: true,
      awaitPromise: true,
    });
    if (r.exceptionDetails) {
      const d =
        (r.exceptionDetails.exception && r.exceptionDetails.exception.description) ||
        r.exceptionDetails.text;
      throw new Error('evaluate failed: ' + d);
    }
    return r.result ? r.result.value : undefined;
  }

  async goto(url) {
    await this.send('Page.navigate', { url });
    await this.waitLoad();
  }

  async waitLoad(timeoutMs = 15000) {
    const t0 = Date.now();
    while (Date.now() - t0 < timeoutMs) {
      try {
        if ((await this.evaluate('document.readyState')) === 'complete') return;
      } catch {
        /* 导航中: 上下文暂不可用 */
      }
      await sleep(50);
    }
    throw new Error('waitLoad timeout');
  }

  // 应用侧全部经 document 级 click 监听分发 (onDocumentClick/按钮), 无
  // isTrusted 依赖 — 合成 DOM click 与真实指针事件走同一处理器.
  async click(selector) {
    const ok = await this.evaluate(
      '(() => { const el = document.querySelector(' + JSON.stringify(selector) + '); if (!el) return false; el.click(); return true; })()'
    );
    if (!ok) throw new Error('click: no element for ' + selector);
  }

  // 非文本键: rawKeyDown + keyUp (与 Playwright keyboard.press 等价的 CDP 序列)
  async press(key, code, vk) {
    const base = { key, code, windowsVirtualKeyCode: vk, nativeVirtualKeyCode: vk };
    await this.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...base });
    await this.send('Input.dispatchKeyEvent', { type: 'keyUp', ...base });
  }

  async close() {
    for (const [method, handler] of this._listeners) {
      this.cdp.remove(method, handler);
    }
    this._listeners = [];
    try {
      await this.cdp.send('Target.closeTarget', { targetId: this.targetId });
    } catch {
      /* 目标已销毁 */
    }
  }
}

/* ---------- fixture / server / chrome 生命周期 ---------- */

function makeFixture() {
  const root = mkdtempSync(join(tmpdir(), 'cb-pdf-accept-fixture-'));
  writeFileSync(join(root, 'a.md'), '# A\n\npage a.\n');
  writeFileSync(join(root, 'doc.pdf'), Buffer.from('%PDF-1.4 fake pdf bytes'));
  mkdirSync(join(root, 'folder.pdf'));
  writeFileSync(join(root, 'folder.pdf', 'note.md'), '# Note\n\nnote page.\n');
  return root;
}

function probePort(port) {
  return new Promise((resolve) => {
    const srv = net.createServer();
    srv.once('error', () => resolve(false));
    srv.once('listening', () => {
      srv.close(() => resolve(true));
    });
    srv.listen(port, '127.0.0.1');
  });
}

async function pickPort() {
  for (const port of [7907, 7908, 7909]) {
    if (await probePort(port)) return port;
  }
  throw new Error('no free fixed test port (7907-7909)');
}

let server = null;
let chrome = null;
let chromeProfile = null;
let fixture = null;

function cleanup() {
  if (server) {
    try {
      server.kill('SIGTERM');
    } catch {
      /* 已退出 */
    }
    server = null;
  }
  if (chrome) {
    try {
      chrome.kill('SIGTERM');
    } catch {
      /* 已退出 */
    }
    chrome = null;
  }
  if (chromeProfile) {
    let lastErr = null;
    for (let i = 0; i < 10; i++) {
      try {
        rmSync(chromeProfile, { recursive: true, force: true });
        lastErr = null;
        break;
      } catch (err) {
        lastErr = err;
        Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 100);
      }
    }
    if (lastErr) {
      console.error('WARN: chrome profile cleanup failed (left for OS): ' + lastErr.message);
    }
    chromeProfile = null;
  }
  if (fixture) {
    rmSync(fixture, { recursive: true, force: true });
    fixture = null;
  }
}

process.on('exit', cleanup);
process.on('SIGINT', () => {
  cleanup();
  process.exit(130);
});
process.on('SIGTERM', () => {
  cleanup();
  process.exit(143);
});

/* ---------- 启动: fixture + server + chrome + CDP ---------- */

fixture = makeFixture();
const port = await pickPort();
const BIN = join(import.meta.dirname, '..', 'target', 'release', 'chapbook');
if (!existsSync(BIN)) {
  throw new Error('missing ' + BIN + ' — run `cargo build --release` first');
}
server = spawn(BIN, ['--port', String(port), fixture], { stdio: ['ignore', 'pipe', 'pipe'] });
let serverErr = '';
server.stderr.on('data', (d) => {
  serverErr += d.toString();
});
{
  let ready = false;
  const t0 = Date.now();
  while (Date.now() - t0 < 15000) {
    if (server.exitCode !== null) {
      throw new Error('chapbook exited early (' + server.exitCode + '): ' + serverErr.slice(-2000));
    }
    try {
      const res = await fetch('http://127.0.0.1:' + port + '/');
      if (res.ok) {
        ready = true;
        break;
      }
    } catch {
      /* 未就绪 */
    }
    await sleep(100);
  }
  if (!ready) {
    throw new Error('chapbook did not become ready on port ' + port + ': ' + serverErr.slice(-2000));
  }
}
const BASE = 'http://127.0.0.1:' + port;

const chromePath = findChrome();
if (!chromePath) {
  throw new Error('no Chrome found — set CHROME_BIN or install ms-playwright chromium');
}
chromeProfile = mkdtempSync(join(tmpdir(), 'cb-pdf-accept-chrome-'));
const headlessFlag = chromePath.includes('headless_shell') ? [] : ['--headless=new'];
chrome = spawn(
  chromePath,
  [
    ...headlessFlag,
    '--remote-debugging-port=0',
    '--user-data-dir=' + chromeProfile,
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-background-networking',
    '--disable-component-update',
    '--disable-default-apps',
    '--disable-sync',
    '--metrics-recording-only',
    ...(process.platform === 'linux' ? ['--no-sandbox', '--disable-dev-shm-usage'] : []),
    'about:blank',
  ],
  { stdio: ['ignore', 'ignore', 'pipe'] }
);
let chromeErr = '';
chrome.stderr.on('data', (d) => {
  chromeErr += d.toString();
});
const portFile = join(chromeProfile, 'DevToolsActivePort');
let cdpPort = null;
let wsPath = null;
{
  const t0 = Date.now();
  while (Date.now() - t0 < 20000) {
    if (chrome.exitCode !== null) {
      throw new Error('chrome exited early (' + chrome.exitCode + '): ' + chromeErr.slice(-2000));
    }
    try {
      const txt = readFileSync(portFile, 'utf8').split('\n');
      if (txt[0]) {
        cdpPort = parseInt(txt[0], 10);
        wsPath = txt[1] || '';
        break;
      }
    } catch {
      /* 端口文件未就绪 */
    }
    await sleep(100);
  }
  if (cdpPort === null) {
    throw new Error('chrome DevToolsActivePort not ready: ' + chromeErr.slice(-2000));
  }
}
const browserCdp = new Cdp('ws://127.0.0.1:' + cdpPort + wsPath);
await browserCdp.open();

/* ---------- 通用场景工具 ---------- */

// window.open spy: 在新文档创建前注入 (Page.addScriptToEvaluateOnNewDocument),
// 拦截一切 window.open 调用并记录 (url/name/features), 返回 null (调用方不使用
// 返回值)。每场景 fresh target = 全新 window, 各自独立 spy。
const OPEN_SPY_SOURCE =
  '(() => { window.__openSpy = []; window.open = function (url, name, features) {' +
  ' window.__openSpy.push({ url: String(url), name: name, features: features }); return null; }; })();';

let activePage = null;

async function fresh(mode) {
  const { targetId } = await browserCdp.send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await browserCdp.send('Target.attachToTarget', {
    targetId,
    flatten: true,
  });
  const page = new Page(browserCdp, targetId, sessionId);
  activePage = page; // 先于任何可能抛出的步骤登记: fresh 失败同样走 finally 关闭
  const pageErrors = [];
  const onException = (params, sid) => {
    if (sid !== sessionId) return;
    const d = params.exceptionDetails;
    pageErrors.push('pageerror: ' + (d.exception ? d.exception.description : d.text));
  };
  page.on('Runtime.exceptionThrown', onException);
  await page.send('Runtime.enable');
  await page.send('Page.enable');
  await page.send('Network.enable');
  await page.send('Page.addScriptToEvaluateOnNewDocument', {
    source: OPEN_SPY_SOURCE,
  });
  // 固定宽屏视口: 默认 headless 窗口可能 <64rem 触发窄屏抽屉 (树 off-canvas).
  await page.send('Emulation.setDeviceMetricsOverride', {
    width: 1280,
    height: 800,
    deviceScaleFactor: 0,
    mobile: false,
  });
  // 进入页面前先清空 origin 的 localStorage (cb-mode 等场景间不泄漏).
  await page.send('Storage.clearDataForOrigin', {
    origin: BASE,
    storageTypes: 'local_storage',
  });
  await page.goto(BASE + '/');
  if (mode === 'tree') {
    if (!(await page.evaluate("document.body.classList.contains('cb-tree-mode')"))) {
      await page.click('.cb-mode-toggle');
    }
    await waitFor(
      () => page.evaluate("!!document.querySelector('ul#cb-tree-root > li.cb-tree-node')"),
      'root attached',
      10000
    );
    await waitFor(() => page.evaluate("!!document.querySelector('.cb-pane')"), 'pane ready', 10000);
    await sleep(400); // 根 JSON settle
  }
  return { page, pageErrors };
}

async function waitFor(fn, label = '?', timeoutMs = 8000, step = 50) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    try {
      const v = await fn();
      if (v) return v;
    } catch {
      /* 元素未就绪 */
    }
    await sleep(step);
  }
  throw new Error('waitFor timeout: ' + label);
}

async function runScenario(name, fn) {
  try {
    await fn();
  } catch (e) {
    rec(name, false, 'THREW: ' + String(e && e.message ? e.message : e));
  } finally {
    if (activePage) {
      const page = activePage;
      activePage = null;
      try {
        await page.close();
      } catch (e) {
        rec(name, false, 'TEARDOWN THREW: ' + String(e && e.message ? e.message : e));
      }
    }
  }
}

const spyState = (page) =>
  page.evaluate('(() => ({ n: window.__openSpy.length, calls: window.__openSpy }))()');

const panelHidden = (page) =>
  page.evaluate('(() => { const p = document.getElementById("cb-preview"); return p ? p.hidden : null; })()');

/* =====================================================================
 * 1. 目录模式行 ◈ 预览 PDF: spy 收到完整 Full URL (noopener), 面板不打开,
 *    URL 不变, 零页面错误.
 * ===================================================================== */
await runScenario('1 dir-mode row preview of PDF -> window.open full URL, panel stays closed', async () => {
  const { page, pageErrors } = await fresh('dir');
  await page.click('.cb-row[data-native-path-encoded="doc.pdf"] [data-cb-action="preview"]');
  await sleep(300);
  const spy = await spyState(page);
  const hidden = await panelHidden(page);
  const path = await page.evaluate('location.pathname');
  const ok =
    spy.n === 1 &&
    spy.calls[0].url === BASE + '/doc.pdf' &&
    spy.calls[0].name === '_blank' &&
    spy.calls[0].features === 'noopener' &&
    hidden === true &&
    path === '/' &&
    pageErrors.length === 0;
  rec(
    '1 dir-mode row preview of PDF -> window.open full URL, panel stays closed',
    ok,
    ' spy=' + JSON.stringify(spy.calls) + ' hidden=' + hidden + ' path=' + path +
      ' errors=' + JSON.stringify(pageErrors)
  );
});

/* =====================================================================
 * 2. 树模式点击 PDF 节点: spy 收到 URL, pane 保持先前内容 (a.md), URL 不变,
 *    零页面错误.
 * ===================================================================== */
await runScenario('2 tree-mode click PDF node -> window.open, pane keeps previous content', async () => {
  const { page, pageErrors } = await fresh('tree');
  // 先 SPA 导航到 a.md, 作为 pane 的"先前内容"
  await page.click('[data-native-path-encoded="a.md"] .cb-tree-row');
  await waitFor(() => page.evaluate("location.pathname === '/a.md'"), 'commit a.md', 10000);
  await sleep(300);
  await page.click('[data-native-path-encoded="doc.pdf"] .cb-tree-row');
  await sleep(400);
  const spy = await spyState(page);
  const path = await page.evaluate('location.pathname');
  const paneText = await page.evaluate(
    "(() => { const d = document.querySelector('.cb-pane .cb-doc'); return d ? d.textContent : null; })()"
  );
  const ok =
    spy.n === 1 &&
    spy.calls[0].url === BASE + '/doc.pdf' &&
    spy.calls[0].features === 'noopener' &&
    path === '/a.md' &&
    typeof paneText === 'string' && paneText.includes('page a') &&
    pageErrors.length === 0;
  rec(
    '2 tree-mode click PDF node -> window.open, pane keeps previous content',
    ok,
    ' spy=' + JSON.stringify(spy.calls) + ' path=' + path + ' paneHasA=' + (paneText || '').includes('page a') +
      ' errors=' + JSON.stringify(pageErrors)
  );
});

/* =====================================================================
 * 3. 树模式点击 folder.pdf 目录: 正常展开 (aria-expanded true + 子项可见),
 *    spy 零调用, 零页面错误.
 * ===================================================================== */
await runScenario('3 tree-mode click folder.pdf dir -> toggles normally, zero spy calls', async () => {
  const { page, pageErrors } = await fresh('tree');
  const before = await page.evaluate(
    "(() => { const li = document.querySelector('[data-native-path-encoded=\"folder.pdf\"]'); return li ? li.getAttribute('aria-expanded') : null; })()"
  );
  await page.click('[data-native-path-encoded="folder.pdf"] .cb-tree-row');
  await waitFor(
    () =>
      page.evaluate(
        "!!document.querySelector('[data-native-path-encoded=\"folder.pdf\"] ul.cb-tree-children li')"
      ),
    'folder.pdf expanded',
    10000
  );
  await sleep(300);
  const after = await page.evaluate(
    "(() => { const li = document.querySelector('[data-native-path-encoded=\"folder.pdf\"]'); const ul = li ? li.querySelector(':scope > ul.cb-tree-children') : null; return { expanded: li ? li.getAttribute('aria-expanded') : null, kids: ul ? ul.querySelectorAll(':scope > li').length : -1 }; })()"
  );
  const spy = await spyState(page);
  const ok =
    before === 'false' &&
    after.expanded === 'true' &&
    after.kids === 1 &&
    spy.n === 0 &&
    pageErrors.length === 0;
  rec(
    '3 tree-mode click folder.pdf dir -> toggles normally, zero spy calls',
    ok,
    ' before=' + before + ' after=' + JSON.stringify(after) + ' spyN=' + spy.n +
      ' errors=' + JSON.stringify(pageErrors)
  );
});

/* =====================================================================
 * 4. 树模式面板迷你列表: 点击 PDF 文件 -> spy, 无 SPA 导航 (URL 不变);
 *    点击 folder.pdf 目录 -> SPA 下钻 (URL 变为 /folder.pdf), spy 零调用.
 * ===================================================================== */
await runScenario('4 pane mini-list: PDF file -> window.open; folder.pdf dir -> SPA, zero spy', async () => {
  const { page, pageErrors } = await fresh('tree');
  await waitFor(
    () => page.evaluate("!!document.querySelector('.cb-pane .cb-dir-mini a[data-native-path-encoded=\"doc.pdf\"]')"),
    'pane mini list ready',
    10000
  );
  await page.click('.cb-pane .cb-dir-mini a[data-native-path-encoded="doc.pdf"]');
  await sleep(400);
  const spy = await spyState(page);
  const pathAfterPdf = await page.evaluate('location.pathname');
  const okPdf =
    spy.n === 1 &&
    spy.calls[0].url === BASE + '/doc.pdf' &&
    spy.calls[0].features === 'noopener' &&
    pathAfterPdf === '/';

  // 目录: SPA 下钻, spy 零调用
  await page.click('.cb-pane .cb-dir-mini a[data-native-path-encoded="folder.pdf"]');
  await waitFor(() => page.evaluate("location.pathname === '/folder.pdf'"), 'mini dir SPA nav', 10000);
  await sleep(300);
  const spy2 = await spyState(page);
  const ok =
    okPdf &&
    spy2.n === 1 &&
    pageErrors.length === 0;
  rec(
    '4 pane mini-list: PDF file -> window.open; folder.pdf dir -> SPA, zero spy',
    ok,
    ' pdfSpy=' + JSON.stringify(spy.calls) + ' pathAfterPdf=' + pathAfterPdf +
      ' spyAfterDir=' + spy2.n + ' errors=' + JSON.stringify(pageErrors)
  );
});

/* =====================================================================
 * 5. 目录模式 ◈ 预览 folder.pdf 目录: 面板显示目录 fragment (note.md 迷你
 *    链接), spy 零调用, 零页面错误.
 * ===================================================================== */
await runScenario('5 dir-mode preview of folder.pdf dir -> dir fragment in panel, zero spy', async () => {
  const { page, pageErrors } = await fresh('dir');
  await page.click('.cb-row[data-native-path-encoded="folder.pdf"] [data-cb-action="preview"]');
  await waitFor(
    () =>
      page.evaluate(
        "!!document.querySelector('#cb-preview-content .cb-dir-fragment a[data-native-path-encoded=\"folder.pdf/note.md\"]')"
      ),
    'dir fragment in panel',
    10000
  );
  await sleep(300);
  const spy = await spyState(page);
  const hidden = await panelHidden(page);
  const fragOk = await page.evaluate(
    "!!document.querySelector('#cb-preview-content .cb-dir-fragment')"
  );
  const ok =
    spy.n === 0 &&
    hidden === false &&
    fragOk &&
    pageErrors.length === 0;
  rec(
    '5 dir-mode preview of folder.pdf dir -> dir fragment in panel, zero spy',
    ok,
    ' spyN=' + spy.n + ' hidden=' + hidden + ' frag=' + fragOk + ' errors=' + JSON.stringify(pageErrors)
  );
});

/* =====================================================================
 * 6. 目录模式选中 PDF 行后按 p: spy 收到 URL, 面板不打开, 零页面错误.
 * ===================================================================== */
await runScenario('6 p key on selected PDF row -> window.open, panel stays closed', async () => {
  const { page, pageErrors } = await fresh('dir');
  // 点击行本身 (非文件名 anchor) 选中 doc.pdf 行
  await page.click('.cb-row[data-native-path-encoded="doc.pdf"]');
  const selected = await page.evaluate(
    "(() => { const el = document.querySelector('.cb-row.cb-selected'); return el ? el.dataset.nativePathEncoded : null; })()"
  );
  await page.press('p', 'KeyP', 80);
  await sleep(300);
  const spy = await spyState(page);
  const hidden = await panelHidden(page);
  const path = await page.evaluate('location.pathname');
  const ok =
    selected === 'doc.pdf' &&
    spy.n === 1 &&
    spy.calls[0].url === BASE + '/doc.pdf' &&
    spy.calls[0].features === 'noopener' &&
    hidden === true &&
    path === '/' &&
    pageErrors.length === 0;
  rec(
    '6 p key on selected PDF row -> window.open, panel stays closed',
    ok,
    ' selected=' + selected + ' spy=' + JSON.stringify(spy.calls) + ' hidden=' + hidden +
      ' path=' + path + ' errors=' + JSON.stringify(pageErrors)
  );
});

/* =====================================================================
 * 7. 服务端头 (Node fetch, 同 server 进程): /doc.pdf 200 application/pdf
 *    无 CSP + nosniff; ?download=1 sandbox + attachment.
 * ===================================================================== */
{
  const res = await fetch(BASE + '/doc.pdf');
  const csp = res.headers.get('content-security-policy');
  const nosniff = res.headers.get('x-content-type-options');
  const ct = res.headers.get('content-type');
  const body = await res.arrayBuffer();
  const rawOk =
    res.status === 200 &&
    csp === null &&
    nosniff === 'nosniff' &&
    ct === 'application/pdf' &&
    new TextDecoder().decode(body) === '%PDF-1.4 fake pdf bytes';

  const dres = await fetch(BASE + '/doc.pdf?download=1');
  const dcsp = dres.headers.get('content-security-policy');
  const disp = dres.headers.get('content-disposition');
  const dlOk =
    dres.status === 200 &&
    dcsp === 'sandbox allow-scripts' &&
    typeof disp === 'string' && disp.includes('attachment');

  rec(
    '7 server headers: /doc.pdf no CSP + nosniff; ?download=1 sandbox + attachment',
    rawOk && dlOk,
    ' raw={status:' + res.status + ' csp:' + csp + ' nosniff:' + nosniff + ' ct:' + ct + '}' +
      ' download={csp:' + dcsp + ' disposition:' + disp + '}'
  );
}

/* ---------- 收尾: 汇总 + 清理 ---------- */

try {
  const pass = results.filter((r) => r.pass).length;
  console.log('SUMMARY: ' + pass + '/' + results.length);
  if (pass !== results.length) {
    results.filter((r) => !r.pass).forEach((r) => console.log('  FAILED: ' + r.name));
    process.exitCode = 1;
  }
} finally {
  try {
    browserCdp.close();
  } catch {
    /* 已关闭 */
  }
  try {
    cleanup();
  } catch (err) {
    console.error('WARN: cleanup failed: ' + (err && err.message ? err.message : String(err)));
  }
}
