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
//   8  搜索完整页 (list): token meta + leaf.pdf 行 native/preview 控制; 同位置
//      切 grid (replace, 不 reload, URL/history 不变, 网格 section/条目可见);
//      切回 list 点 PDF 预览控制 -> spy 收到编码 URL (网格项无 per-item 控制)
//   9  搜索树视图 -> list/grid 整页交接: q/sort/hidden 原样保持, history 项数
//      不变, token 与 leaf.pdf 行 native 控制恢复; 内容区搜索片段无 native/token
//  10  目录树视图下钻 /adir 后 -> list: URL 保持 /adir (+sort/hidden), 不新增
//      历史项, 整页 token + adir/child.md 行 native 控制
//  11  BFCache: 离开 /a.md 后 history.back() 触发 persisted pageshow, 快照幂等
//      重放 (DOM/history.state.cb 不变), 零 /__/tree|fragment|Document 重取;
//      离开期间改动的偏好不覆盖恢复快照
//  12  跨 tab 偏好隔离: 另一 tab 改 view/sort/hidden 后, 本 tab reload 仍按自身
//      快照恢复 (tree + hidden=1, 隐藏条目可见) 且不覆盖共享偏好
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

  // 真实指针 + 命中检查, 不用 el.click() 绕过遮挡/闭合菜单/不可见控件.
  async click(selector) {
    for (let step = 0; step < 2; step++) {
      const point = await this.evaluate(`(() => {
        const el = document.querySelector(${JSON.stringify(selector)});
        if (!el) throw new Error('click: missing element');
        const details = el.closest('summary') ? null : el.closest('details:not([open])');
        const target = details ? details.querySelector('summary') : el;
        target.scrollIntoView({block:'nearest',inline:'nearest'});
        const r = target.getBoundingClientRect();
        const x = r.left + r.width / 2, y = r.top + r.height / 2;
        const hit = document.elementFromPoint(x, y);
        if (!r.width || !r.height || !hit || !target.contains(hit)) {
          throw new Error('click: invisible or covered element ' + ${JSON.stringify(selector)});
        }
        return {x,y,opening:!!details};
      })()`);
      await this.send('Input.dispatchMouseEvent', {type:'mouseMoved',x:point.x,y:point.y});
      await this.send('Input.dispatchMouseEvent', {type:'mousePressed',x:point.x,y:point.y,button:'left',clickCount:1});
      await this.send('Input.dispatchMouseEvent', {type:'mouseReleased',x:point.x,y:point.y,button:'left',clickCount:1});
      if (!point.opening) return;
    }
    throw new Error('click: menu did not open for ' + selector);
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
  // 搜索命中 (q=leaf) 的 PDF + 隐藏项 (hidden 规则场景).
  writeFileSync(join(root, 'leaf.pdf'), Buffer.from('%PDF-1.4 fake pdf bytes leaf'));
  writeFileSync(join(root, '.hidden.txt'), 'hidden entry\n');
  mkdirSync(join(root, 'folder.pdf'));
  writeFileSync(join(root, 'folder.pdf', 'note.md'), '# Note\n\nnote page.\n');
  mkdirSync(join(root, '.hidden.pdf'));
  writeFileSync(join(root, '.hidden.pdf', 'inside.md'), '# Inside\n\nhidden directory page.\n');
  // 树内下钻 / BFCache 场景的嵌套目录.
  mkdirSync(join(root, 'adir'));
  writeFileSync(join(root, 'adir', 'child.md'), '# Child\n\nchild page.\n');
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

// 同步终止子进程并等其真正退出: Chrome 关闭期间会重建 profile 目录, 必须先
// 等进程消失再删, 否则残留 (cleanup 也可在 'exit' 处理器内同步执行).
function stopChild(child) {
  if (!child) return null;
  for (const signal of ['SIGTERM', 'SIGKILL']) {
    try {
      child.kill(signal);
    } catch {
      /* 已退出 */
    }
    let gone = false;
    for (let i = 0; i < 50; i++) {
      if (child.exitCode !== null) {
        gone = true;
        break;
      }
      try {
        process.kill(child.pid, 0);
      } catch {
        gone = true; // 进程已消失
        break;
      }
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 100);
    }
    if (gone) break;
  }
  return null;
}

function cleanup() {
  server = stopChild(server);
  chrome = stopChild(chrome);
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

// 预 CDP 失败也显式 cleanup: 不依赖进程级 'exit' 行为留下 server/chrome/profile.
function fatal(message) {
  cleanup();
  throw new Error(message);
}

/* ---------- 启动: fixture + server + chrome + CDP ---------- */

// 前置条件先于任何临时资源分配: 缺 binary / 无 Chrome 时不留 fixture 与 profile.
const BIN = process.env.CB_BIN || join(import.meta.dirname, '..', 'target', 'release', 'chapbook');
if (!existsSync(BIN)) {
  throw new Error('missing ' + BIN + ' — run `cargo build --release` first (or set CB_BIN)');
}
const chromePath = findChrome();
if (!chromePath) {
  throw new Error('no Chrome found — set CHROME_BIN or install ms-playwright chromium');
}
const port = await pickPort();

fixture = makeFixture();
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
      fatal('chapbook exited early (' + server.exitCode + '): ' + serverErr.slice(-2000));
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
    fatal('chapbook did not become ready on port ' + port + ': ' + serverErr.slice(-2000));
  }
}
const BASE = 'http://127.0.0.1:' + port;

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
      fatal('chrome exited early (' + chrome.exitCode + '): ' + chromeErr.slice(-2000));
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
    fatal('chrome DevToolsActivePort not ready: ' + chromeErr.slice(-2000));
  }
}
const browserCdp = new Cdp('ws://127.0.0.1:' + cdpPort + wsPath);
try {
  await browserCdp.open();
} catch (err) {
  fatal('browser CDP connect failed: ' + (err && err.message ? err.message : String(err)));
}

/* ---------- 通用场景工具 ---------- */

// window.open spy: 在新文档创建前注入 (Page.addScriptToEvaluateOnNewDocument),
// 拦截一切 window.open 调用并记录 (url/name/features), 返回 null (调用方不使用
// 返回值)。每场景 fresh target = 全新 window, 各自独立 spy。
const OPEN_SPY_SOURCE =
  '(() => { window.__openSpy = []; window.open = function (url, name, features) {' +
  ' window.__openSpy.push({ url: String(url), name: name, features: features }); return null; }; })();';

// BFCache 观察: pageshow 监听器在新文档创建前注入, 记录每次 event.persisted
// (首次 load = false; BFCache 恢复 = true). 注入脚本不拦截任何请求/导航 —
// BFCache 场景绝不启用 Fetch/Network interception.
const PAGESHOW_SOURCE =
  '(() => { window.__pageshowPersisted = [];' +
  " window.addEventListener('pageshow', function (event) { window.__pageshowPersisted.push(!!event.persisted); }); })();";

// 页面状态快照 (BFCache 前后逐字段对比): 语义属性 / 视图按下态 / 隐藏开关 /
// 面板 / 正文可见文本 / history.state.cb / URL / 历史项数.
const READ_STATE_SOURCE =
  '(() => {' +
  ' const views = Array.from(document.querySelectorAll("[data-cb-view]")).map((el) => ({ view: el.getAttribute("data-cb-view"), pressed: el.getAttribute("aria-pressed") }));' +
  ' const toggle = document.querySelector("[data-cb-hidden-toggle]");' +
  ' const panel = document.getElementById("cb-preview");' +
  ' return {' +
  '  surface: document.body.dataset.cbSurface || null,' +
  '  kind: document.body.dataset.kind || null,' +
  '  path: document.body.dataset.nativePathEncoded,' +
  '  sort: document.body.dataset.cbSort || null,' +
  '  hidden: document.body.dataset.cbHidden || null,' +
  '  views: views,' +
  '  hiddenToggle: toggle ? { pressed: toggle.getAttribute("aria-pressed"), disabled: toggle.getAttribute("aria-disabled") } : null,' +
  '  panelHidden: panel ? panel.hidden : null,' +
  '  text: document.body.textContent.replace(/\\s+/g, " ").trim(),' +
  '  cb: history.state && history.state.cb ? JSON.stringify(history.state.cb) : null,' +
  '  href: location.href,' +
  '  len: history.length' +
  ' };' +
  '})()';

let activePage = null;

// 每个 target 的 CDP 初始化 (fresh 与跨 tab 场景共用): 域启用 + spy 注入 +
// 固定宽屏视口 (默认 headless 窗口可能 <64rem 触发窄屏抽屉, 树 off-canvas).
async function createTarget() {
  const { targetId } = await browserCdp.send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await browserCdp.send('Target.attachToTarget', {
    targetId,
    flatten: true,
  });
  const page = new Page(browserCdp, targetId, sessionId);
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
  await page.send('Emulation.setDeviceMetricsOverride', {
    width: 1280,
    height: 800,
    deviceScaleFactor: 0,
    mobile: false,
  });
  return { page, pageErrors };
}

async function fresh(mode) {
  const { page, pageErrors } = await createTarget();
  activePage = page; // 登记活动页: 场景结束 (含失败) 统一在 runScenario finally 关闭
  // 进入页面前先清空 origin 的 localStorage (场景间偏好不泄漏).
  await page.send('Storage.clearDataForOrigin', {
    origin: BASE,
    storageTypes: 'local_storage',
  });
  await page.goto(BASE + '/');
  if (mode === 'tree') {
    // 统一三态视图: 树入口是 [data-cb-view="tree"] (同位置 replace, 不整页重载).
    if (!(await page.evaluate("document.body.classList.contains('cb-tree-mode')"))) {
      await page.click('[data-cb-view="tree"]');
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

// 下一次整页 load (点击前注册: 视图交接 / 规则变化都是整页重载).
function waitForLoad(page, timeoutMs = 15000) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      page.cdp.remove('Page.loadEventFired', h);
      reject(new Error('load event timeout'));
    }, timeoutMs);
    const h = (params, sid) => {
      if (sid !== page.sessionId) return;
      clearTimeout(timer);
      page.cdp.remove('Page.loadEventFired', h);
      resolve();
    };
    page.on('Page.loadEventFired', h);
  });
}

// 导航并等待新文档 load (load 事件先注册, 避免旧文档 readyState 竞态).
async function gotoAndWait(page, url) {
  const loaded = waitForLoad(page);
  await page.send('Page.navigate', { url });
  await loaded;
  await sleep(150);
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

// 本机打开目标 = 同源 + 精确文件 pathname. 查询只承载当前有效显示规则
// (sort/hidden), 属随规则变化的附带状态, 不锁定字面拼写; 必须是文件字节出口
// (无 hash, 无 fragment/view/download 传输参数), 不是 SPA/pane 中间 URL.
function isFileTarget(raw, encodedPath) {
  let url;
  try {
    url = new URL(raw);
  } catch {
    return false;
  }
  return (
    url.origin === BASE &&
    url.pathname === '/' + encodedPath &&
    url.hash === '' &&
    url.searchParams.get('fragment') === null &&
    url.searchParams.get('view') === null &&
    url.searchParams.get('download') === null
  );
}

// 原生新 tab 契约: 目标文件 + _blank + noopener (spy 在 window.open 层观察).
function isNativeFileOpen(call, encodedPath) {
  return call.name === '_blank' && call.features === 'noopener' && isFileTarget(call.url, encodedPath);
}

// BFCache 快照整体一致: history.length 在 back 恢复后不回落 (离开导航已 +1),
// 故逐字段整体比较排除 len; 其余字段 (含 history.state.cb) 必须逐字节一致.
const sameStateExceptHistoryLength = (a, b) =>
  JSON.stringify({ ...a, len: 0 }) === JSON.stringify({ ...b, len: 0 });

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
    isNativeFileOpen(spy.calls[0], 'doc.pdf') &&
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
    isNativeFileOpen(spy.calls[0], 'doc.pdf') &&
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
    isNativeFileOpen(spy.calls[0], 'doc.pdf') &&
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
    isNativeFileOpen(spy.calls[0], 'doc.pdf') &&
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

/* =====================================================================
 * 8 搜索完整页 (list) native-open 能力: token meta + leaf.pdf 行 native/preview
 *   控制; 同位置切 grid (replace, 不 reload, URL/history 不变, 网格 section 与
 *   leaf.pdf 网格项可见, 信任页 token/native 仍在); 切回 list 点击 PDF 预览
 *   控制 -> spy 收到编码 URL. 网格项本身无 per-item action 控制 (客户端契约).
 * ===================================================================== */
await runScenario('8 search full page: token + row native control, grid switch same-location, PDF preview window.open', async () => {
  const { page, pageErrors } = await fresh('dir');
  await gotoAndWait(page, BASE + '/__/search?q=leaf');
  const list = await page.evaluate(`(() => {
    const row = document.querySelector('tr.cb-row[data-native-path-encoded="leaf.pdf"]');
    const g = document.getElementById('cb-view-grid');
    const views = {};
    document.querySelectorAll('[data-cb-view]').forEach((el) => {
      views[el.getAttribute('data-cb-view')] = {
        pressed: el.getAttribute('aria-pressed'),
        disabled: el.getAttribute('aria-disabled'),
      };
    });
    return {
      surface: document.body.dataset.cbSurface,
      query: document.body.dataset.cbQuery,
      token: !!document.querySelector('meta[name="cb-native-open-token"]'),
      rows: document.querySelectorAll('tr.cb-row[data-browser-entry]').length,
      native: !!(row && row.querySelector('[data-cb-action="native"]')),
      preview: !!(row && row.querySelector('[data-cb-action="preview"]')),
      rowVisible: row ? row.getClientRects().length > 0 : false,
      gridHidden: g ? g.hidden : null,
      search: location.search,
      len: history.length,
      views: views,
    };
  })()`);
  await page.evaluate("window.__s8 = 'list'");
  await page.click('[data-cb-view="grid"]');
  await sleep(250);
  const grid = await page.evaluate(`(() => {
    const g = document.getElementById('cb-view-grid');
    const l = document.getElementById('cb-view-list');
    const item = document.querySelector('#cb-view-grid div.cb-grid-item[data-native-path-encoded="leaf.pdf"]');
    const rect = item ? item.getBoundingClientRect() : null;
    return {
      gridHidden: g ? g.hidden : null,
      listHidden: l ? l.hidden : null,
      itemVisible: !!(rect && rect.width > 0 && rect.height > 0),
      gridSectionNative: g ? g.querySelectorAll('[data-cb-action="native"]').length : -1,
      pageNative: document.querySelectorAll('[data-cb-action="native"]').length,
      token: !!document.querySelector('meta[name="cb-native-open-token"]'),
      gridPressed: (document.querySelector('[data-cb-view="grid"]') || {}).getAttribute('aria-pressed'),
      marker: window.__s8 || null,
      search: location.search,
      len: history.length,
    };
  })()`);
  // 回到 list (同位置 replace, 不 reload) 后点击 PDF 预览控制: spy 收到编码 URL.
  await page.click('[data-cb-view="list"]');
  await sleep(250);
  await page.click('tr.cb-row[data-native-path-encoded="leaf.pdf"] [data-cb-action="preview"]');
  await sleep(300);
  const spy = await spyState(page);
  const hidden = await panelHidden(page);
  const after = await page.evaluate(
    "({ search: location.search, len: history.length, marker: window.__s8 || null })"
  );
  const ok =
    list.surface === 'search' &&
    list.query === 'leaf' &&
    list.token &&
    list.rows >= 1 &&
    list.native &&
    list.preview &&
    list.rowVisible &&
    list.gridHidden === true &&
    list.views.list.pressed === 'true' &&
    list.views.tree.disabled !== 'true' &&
    list.views.grid.disabled !== 'true' &&
    grid.gridHidden === false &&
    grid.listHidden === true &&
    grid.itemVisible &&
    grid.token &&
    grid.pageNative >= 1 &&
    grid.gridPressed === 'true' &&
    grid.marker === 'list' &&
    grid.search === list.search &&
    grid.len === list.len &&
    after.marker === 'list' &&
    after.search === grid.search &&
    after.len === grid.len &&
    spy.n === 1 &&
    isNativeFileOpen(spy.calls[0], 'leaf.pdf') &&
    hidden === true &&
    pageErrors.length === 0;
  rec(
    '8 search full page: token + row native control, grid switch same-location, PDF preview window.open',
    ok,
    ' list=' + JSON.stringify(list) + ' grid=' + JSON.stringify(grid) + ' after=' + JSON.stringify(after) +
      ' spy=' + JSON.stringify(spy.calls) + ' panelHidden=' + hidden +
      ' errors=' + JSON.stringify(pageErrors)
  );
});

/* =====================================================================
 * 9 搜索树视图 -> list/grid 整页交接: [data-cb-view="tree"] 同位置 replace
 *   (pane 出现 .cb-search-fragment, 片段内无 native/token, 不 reload); 切
 *   list -> 整页交接保持 q/sort/hidden 与 history 项数, token + leaf.pdf 行
 *   native 控制恢复; 再进 tree 后切 grid 同样成立.
 * ===================================================================== */
await runScenario('9 search tree view: pane fragment token-free, list/grid handoff keeps q/sort/hidden and history', async () => {
  const { page, pageErrors } = await fresh('dir');
  await gotoAndWait(page, BASE + '/__/search?q=leaf&sort=Name:Asc&hidden=0');
  const before = await page.evaluate(
    "({ search: location.search, len: history.length, token: !!document.querySelector('meta[name=\"cb-native-open-token\"]'), rows: document.querySelectorAll('tr.cb-row[data-browser-entry]').length })"
  );
  await page.evaluate("window.__s9 = 'list'");
  await page.click('[data-cb-view="tree"]');
  await waitFor(
    () => page.evaluate("!!document.querySelector('.cb-pane .cb-search-fragment')"),
    'search fragment in pane',
    10000
  );
  await sleep(250);
  const tree = await page.evaluate(`(() => {
    const f = document.querySelector('.cb-pane .cb-search-fragment');
    return {
      treeMode: document.body.classList.contains('cb-tree-mode'),
      marker: window.__s9 || null,
      search: location.search,
      len: history.length,
      query: f.getAttribute('data-cb-query'),
      surface: f.getAttribute('data-cb-surface'),
      nativeInPane: f.querySelectorAll('[data-cb-action="native"]').length,
      tokenInPane: !!f.querySelector('meta[name="cb-native-open-token"]'),
      tokenInDocument: !!document.querySelector('meta[name="cb-native-open-token"]'),
      treePressed: (document.querySelector('[data-cb-view="tree"]') || {}).getAttribute('aria-pressed'),
    };
  })()`);
  const loaded = waitForLoad(page);
  await page.click('[data-cb-view="list"]');
  await loaded;
  await sleep(300);
  const listAfter = await page.evaluate(`(() => {
    const row = document.querySelector('tr.cb-row[data-native-path-encoded="leaf.pdf"]');
    return {
      marker: window.__s9 || null,
      path: location.pathname,
      search: location.search,
      len: history.length,
      token: !!document.querySelector('meta[name="cb-native-open-token"]'),
      native: !!(row && row.querySelector('[data-cb-action="native"]')),
      rows: document.querySelectorAll('tr.cb-row[data-browser-entry]').length,
      rowVisible: row ? row.getClientRects().length > 0 : false,
      listPressed: (document.querySelector('[data-cb-view="list"]') || {}).getAttribute('aria-pressed'),
    };
  })()`);
  // 再次进入 tree, 然后切 grid: 同样 replace+reload, 查询与历史项数不变.
  await page.evaluate("window.__s9b = 'list'");
  await page.click('[data-cb-view="tree"]');
  await waitFor(
    () => page.evaluate("!!document.querySelector('.cb-pane .cb-search-fragment')"),
    'search fragment in pane (2nd)',
    10000
  );
  await sleep(250);
  const treeAgain = await page.evaluate(
    "({ marker: window.__s9b || null, search: location.search, len: history.length })"
  );
  const loaded2 = waitForLoad(page);
  await page.click('[data-cb-view="grid"]');
  await loaded2;
  await sleep(300);
  const gridAfter = await page.evaluate(`(() => {
    const g = document.getElementById('cb-view-grid');
    const item = document.querySelector('#cb-view-grid div.cb-grid-item[data-native-path-encoded="leaf.pdf"]');
    const rect = item ? item.getBoundingClientRect() : null;
    return {
      marker: window.__s9b || null,
      path: location.pathname,
      search: location.search,
      len: history.length,
      token: !!document.querySelector('meta[name="cb-native-open-token"]'),
      gridHidden: g ? g.hidden : null,
      itemVisible: !!(rect && rect.width > 0 && rect.height > 0),
      gridPressed: (document.querySelector('[data-cb-view="grid"]') || {}).getAttribute('aria-pressed'),
    };
  })()`);
  const keepQuery = (search) => {
    const p = new URLSearchParams(search);
    return p.get('q') === 'leaf' && p.get('sort') === 'Name:Asc' && p.get('hidden') === '0';
  };
  const ok =
    keepQuery(before.search) &&
    before.token &&
    before.rows >= 1 &&
    tree.treeMode &&
    tree.marker === 'list' &&
    tree.search === before.search &&
    tree.len === before.len &&
    tree.query === 'leaf' &&
    tree.surface === 'search' &&
    tree.nativeInPane === 0 &&
    tree.tokenInPane === false &&
    tree.treePressed === 'true' &&
    listAfter.marker === null &&
    listAfter.path === '/__/search' &&
    keepQuery(listAfter.search) &&
    listAfter.len === before.len &&
    listAfter.token &&
    listAfter.native &&
    listAfter.rows >= 1 &&
    listAfter.rowVisible &&
    listAfter.listPressed === 'true' &&
    treeAgain.marker === 'list' &&
    keepQuery(treeAgain.search) &&
    treeAgain.len === listAfter.len &&
    gridAfter.marker === null &&
    gridAfter.path === '/__/search' &&
    keepQuery(gridAfter.search) &&
    gridAfter.len === treeAgain.len &&
    gridAfter.token &&
    gridAfter.gridHidden === false &&
    gridAfter.itemVisible &&
    gridAfter.gridPressed === 'true' &&
    pageErrors.length === 0;
  rec(
    '9 search tree view: pane fragment token-free, list/grid handoff keeps q/sort/hidden and history',
    ok,
    ' before=' + JSON.stringify(before) + ' tree=' + JSON.stringify(tree) + ' list=' + JSON.stringify(listAfter) +
      ' treeAgain=' + JSON.stringify(treeAgain) + ' grid=' + JSON.stringify(gridAfter) +
      ' errors=' + JSON.stringify(pageErrors)
  );
});

/* =====================================================================
 * 10 目录树视图下钻 /adir 后 -> list 恢复: URL 保持 /adir (+sort/hidden),
 *    history 项数不变 (replace+reload, 不新增项), 整页 token +
 *    adir/child.md 行 native 控制, 列表视图激活.
 * ===================================================================== */
await runScenario('10 directory tree->list at /adir: URL/history unchanged, full page token + native restored', async () => {
  const { page, pageErrors } = await fresh('tree');
  await waitFor(
    () => page.evaluate("!!document.querySelector('.cb-pane .cb-dir-mini a[data-native-path-encoded=\"adir\"]')"),
    'pane mini list ready',
    10000
  );
  await page.click('.cb-pane .cb-dir-mini a[data-native-path-encoded="adir"]');
  await waitFor(() => page.evaluate("location.pathname === '/adir'"), 'pane nav to /adir', 10000);
  await waitFor(
    () => page.evaluate("!!document.querySelector('.cb-pane .cb-dir-mini a[data-native-path-encoded=\"adir/child.md\"]')"),
    'adir fragment in pane',
    10000
  );
  const before = await page.evaluate(
    "({ path: location.pathname, search: location.search, len: history.length })"
  );
  await page.evaluate("window.__s10 = 'tree'");
  const loaded = waitForLoad(page);
  await page.click('[data-cb-view="list"]');
  await loaded;
  await sleep(300);
  const after = await page.evaluate(`(() => {
    const row = document.querySelector('tr.cb-row[data-native-path-encoded="adir/child.md"]');
    return {
      marker: window.__s10 || null,
      path: location.pathname,
      search: location.search,
      len: history.length,
      surface: document.body.dataset.cbSurface,
      kind: document.body.dataset.kind,
      token: !!document.querySelector('meta[name="cb-native-open-token"]'),
      native: !!(row && row.querySelector('[data-cb-action="native"]')),
      rowVisible: row ? row.getClientRects().length > 0 : false,
      listPressed: (document.querySelector('[data-cb-view="list"]') || {}).getAttribute('aria-pressed'),
    };
  })()`);
  const p = new URLSearchParams(after.search);
  const ok =
    before.path === '/adir' &&
    after.marker === null &&
    after.path === '/adir' &&
    after.search === before.search &&
    p.get('sort') === 'Name:Asc' &&
    p.get('hidden') === '0' &&
    after.len === before.len &&
    after.surface === 'directory' &&
    after.kind === 'dir' &&
    after.token &&
    after.native &&
    after.rowVisible &&
    after.listPressed === 'true' &&
    pageErrors.length === 0;
  rec(
    '10 directory tree->list at /adir: URL/history unchanged, full page token + native restored',
    ok,
    ' before=' + JSON.stringify(before) + ' after=' + JSON.stringify(after) +
      ' errors=' + JSON.stringify(pageErrors)
  );
});

/* =====================================================================
 * 11 BFCache: /a.md (reading) -> /adir/child.md -> history.back() 恢复.
 *    pageshow 记录 persisted=true; DOM 与 history.state.cb 与离开前逐字段
 *    一致 (back 恢复后 history.length 不回落, 单独断言恢复未新增项);
 *    back() 后零 /__/tree|fragment=1|Document 请求; 离开期间在另一
 *    文档写入的偏好 (view=grid, hidden 翻转) 不得覆盖恢复快照.
 *    本场景不启用任何 Fetch/Network interception (仅被动 request 监听).
 * ===================================================================== */
await runScenario('11 BFCache: back to /a.md restores reading snapshot with zero tree/fragment/document refetch', async () => {
  const { page, pageErrors } = await fresh('dir');
  await page.send('Page.addScriptToEvaluateOnNewDocument', { source: PAGESHOW_SOURCE });
  await gotoAndWait(page, BASE + '/a.md');
  await sleep(300); // init settle (TOC/公式/图表)
  const before = await page.evaluate(READ_STATE_SOURCE);
  await gotoAndWait(page, BASE + '/adir/child.md');
  const awayLen = await page.evaluate('history.length'); // back 不减少项数: 单独断言恢复未新增项
  // 离开期间另一文档写偏好 (等价另一 tab): 恢复不得重读偏好.
  const flipped = before.hidden === '1' ? '0' : '1';
  await page.evaluate(
    "localStorage.setItem('cb-browse-view', 'grid'); localStorage.setItem('cb-hidden', " + JSON.stringify(flipped) + ");"
  );
  const requests = [];
  page.on('Network.requestWillBeSent', (params, sid) => {
    if (sid === page.sessionId) requests.push({ url: params.request.url, type: params.type });
  });
  await page.evaluate('history.back()');
  try {
    await waitFor(
      () => page.evaluate('Array.isArray(window.__pageshowPersisted) && window.__pageshowPersisted.indexOf(true) >= 0'),
      'bfcache persisted pageshow',
      8000
    );
  } catch {
    /* 观察超时: 稍后按持久化标志断言失败, facts 记录实际观测值 */
  }
  await sleep(250); // 若发生整页 reload 竞态, 让新文档落定后再采样
  const after = await page.evaluate(READ_STATE_SOURCE);
  const persisted = await page.evaluate('window.__pageshowPersisted || null');
  const refetch = requests.filter((r) => r.url.includes('/__/tree') || r.url.includes('fragment=1'));
  const documents = requests.filter((r) => r.type === 'Document');
  const ok =
    Array.isArray(persisted) &&
    persisted.indexOf(true) >= 0 &&
    after.surface === 'reading' &&
    after.kind === 'file' &&
    after.path === before.path &&
    after.sort === before.sort &&
    after.hidden === before.hidden &&
    after.href === before.href &&
    awayLen === before.len + 1 &&
    after.len === awayLen &&
    after.cb === before.cb &&
    after.panelHidden === before.panelHidden &&
    JSON.stringify(after.hiddenToggle) === JSON.stringify(before.hiddenToggle) &&
    after.views.length === 3 &&
    after.views.every((v) => v.pressed === 'false') &&
    typeof after.text === 'string' &&
    after.text.includes('page a') &&
    sameStateExceptHistoryLength(after, before) &&
    refetch.length === 0 &&
    documents.length === 0 &&
    pageErrors.length === 0;
  rec(
    '11 BFCache: back to /a.md restores reading snapshot with zero tree/fragment/document refetch',
    ok,
    ' persisted=' + JSON.stringify(persisted) + ' refetch=' + refetch.length + ' documents=' + documents.length +
      ' surface=' + after.surface + ' kind=' + after.kind + ' pathSame=' + (after.path === before.path) +
      ' sortSame=' + (after.sort === before.sort) + ' hiddenSame=' + (after.hidden === before.hidden) +
      ' len=' + before.len + '->' + awayLen + '->' + after.len +
      ' hrefSame=' + (after.href === before.href) + ' cbSame=' + (after.cb === before.cb) +
      ' textSame=' + (after.text === before.text) +
      ' toggleSame=' + (JSON.stringify(after.hiddenToggle) === JSON.stringify(before.hiddenToggle)) +
      ' views=' + JSON.stringify(after.views) + ' errors=' + JSON.stringify(pageErrors)
  );
});

/* =====================================================================
 * 12 跨 tab 偏好隔离: tab A 进入 tree 并打开 hidden 规则 (隐藏条目可见);
 *    tab B (同浏览器, 独立 target) 改为 grid / Size:Desc / hidden=0 并写入
 *    共享偏好; 回到 A reload: A 必须按自身 managed 快照恢复 (tree + hidden=1,
 *    隐藏条目仍可见, URL/history.state.cb 不变), 且不覆盖共享偏好.
 * ===================================================================== */
await runScenario('12 prefs isolation: tab A reload restores own tree+hidden snapshot despite other tab prefs', async () => {
  const tabA = await fresh('tree');
  const a = tabA.page;
  await a.click('[data-cb-hidden-toggle]');
  await waitFor(
    () => a.evaluate("(() => { const t = document.querySelector('[data-cb-hidden-toggle]'); return !!t && t.getAttribute('aria-pressed') === 'true'; })()"),
    'A hidden toggle on',
    10000
  );
  await waitFor(
    () => a.evaluate("!!document.querySelector('.cb-pane .cb-dir-mini a[data-native-path-encoded=\".hidden.txt\"]')"),
    'A pane shows hidden entry',
    10000
  );
  const aBefore = await a.evaluate(`(() => {
    const t = document.querySelector('[data-cb-hidden-toggle]');
    return {
      href: location.href,
      search: location.search,
      len: history.length,
      treeMode: document.body.classList.contains('cb-tree-mode'),
      treePressed: (document.querySelector('[data-cb-view="tree"]') || {}).getAttribute('aria-pressed'),
      toggle: t ? t.getAttribute('aria-pressed') : null,
      bodyHidden: document.body.dataset.cbHidden || null,
      cb: history.state && history.state.cb ? JSON.stringify(history.state.cb) : null,
      prefs: {
        view: localStorage.getItem('cb-browse-view'),
        sort: localStorage.getItem('cb-sort'),
        hidden: localStorage.getItem('cb-hidden'),
      },
    };
  })()`);
  const tabB = await createTarget();
  try {
    const b = tabB.page;
    await gotoAndWait(b, BASE + '/');
    // B 先继承 A 已保存的共享偏好 (tree + hidden=1). 控件变化里只有 tree -> grid
    // 是交接 reload, 排序/隐藏都是同位置就地刷新: 等待可观察终态而非 load 事件.
    await b.click('[data-cb-view="grid"]');
    await waitFor(
      () =>
        b.evaluate(
          "!document.body.classList.contains('cb-tree-mode') && document.body.dataset.cbSurface === 'directory' &&" +
            " (document.querySelector('[data-cb-view=\"grid\"]') || {}).getAttribute('aria-pressed') === 'true' &&" +
            " (() => { const g = document.getElementById('cb-view-grid'); return !!g && !g.hidden; })()"
        ),
      'B grid view active',
      15000
    );
    await sleep(250);
    await b.click('summary[data-cb-sort-summary]');
    await b.click('[data-cb-sort="Size:Desc"]');
    await waitFor(
      () =>
        b.evaluate(
          "document.body.dataset.cbSort === 'Size:Desc' && !document.querySelector('[data-cb-listing-status]') &&" +
            " (() => { const g = document.getElementById('cb-view-grid'); return !!g && !g.hidden; })()"
        ),
      'B sort Size:Desc applied in grid',
      15000
    );
    await sleep(250);
    await b.click('[data-cb-hidden-toggle]');
    await waitFor(
      () =>
        b.evaluate(
          "document.body.dataset.cbHidden === '0' && !document.querySelector('[data-cb-listing-status]') &&" +
            " (() => { const t = document.querySelector('[data-cb-hidden-toggle]'); return !!t && t.getAttribute('aria-pressed') === 'false'; })()"
        ),
      'B hidden=0 applied',
      15000
    );
    await sleep(250);
    const bAfter = await b.evaluate(`(() => {
      const t = document.querySelector('[data-cb-hidden-toggle]');
      return {
        sort: document.body.dataset.cbSort || null,
        bodyHidden: document.body.dataset.cbHidden || null,
        gridPressed: (document.querySelector('[data-cb-view="grid"]') || {}).getAttribute('aria-pressed'),
        toggle: t ? t.getAttribute('aria-pressed') : null,
        prefs: {
          view: localStorage.getItem('cb-browse-view'),
          sort: localStorage.getItem('cb-sort'),
          hidden: localStorage.getItem('cb-hidden'),
        },
      };
    })()`);
    // 共享 localStorage 现已是 B 的偏好; A 的 reload 必须按自身快照恢复.
    const shared = await a.evaluate(
      "({ view: localStorage.getItem('cb-browse-view'), sort: localStorage.getItem('cb-sort'), hidden: localStorage.getItem('cb-hidden') })"
    );
    const aLoaded = waitForLoad(a);
    await a.send('Page.reload', {});
    await aLoaded;
    await waitFor(
      () => a.evaluate("!!document.querySelector('ul#cb-tree-root > li.cb-tree-node')"),
      'A tree root after reload',
      10000
    );
    await waitFor(
      () => a.evaluate("!!document.querySelector('.cb-pane .cb-dir-mini a[data-native-path-encoded=\".hidden.txt\"]')"),
      'A pane hidden entry after reload',
      10000
    );
    await sleep(200);
    const aAfter = await a.evaluate(`(() => {
      const t = document.querySelector('[data-cb-hidden-toggle]');
      return {
        href: location.href,
        search: location.search,
        len: history.length,
        treeMode: document.body.classList.contains('cb-tree-mode'),
        treePressed: (document.querySelector('[data-cb-view="tree"]') || {}).getAttribute('aria-pressed'),
        toggle: t ? t.getAttribute('aria-pressed') : null,
        bodyHidden: document.body.dataset.cbHidden || null,
        cb: history.state && history.state.cb ? JSON.stringify(history.state.cb) : null,
        prefs: {
          view: localStorage.getItem('cb-browse-view'),
          sort: localStorage.getItem('cb-sort'),
          hidden: localStorage.getItem('cb-hidden'),
        },
      };
    })()`);
    const aBeforeParams = new URLSearchParams(aBefore.search);
    const aAfterParams = new URLSearchParams(aAfter.search);
    const ok =
      aBefore.treeMode &&
      aBefore.treePressed === 'true' &&
      aBefore.toggle === 'true' &&
      aBeforeParams.get('hidden') === '1' &&
      aBefore.prefs.view === 'tree' &&
      aBefore.prefs.hidden === '1' &&
      bAfter.sort === 'Size:Desc' &&
      bAfter.bodyHidden === '0' &&
      bAfter.gridPressed === 'true' &&
      bAfter.toggle === 'false' &&
      bAfter.prefs.view === 'grid' &&
      bAfter.prefs.sort === 'Size:Desc' &&
      bAfter.prefs.hidden === '0' &&
      shared.view === 'grid' &&
      shared.sort === 'Size:Desc' &&
      shared.hidden === '0' &&
      aAfter.treeMode &&
      aAfter.treePressed === 'true' &&
      aAfter.toggle === 'true' &&
      aAfter.bodyHidden === '1' &&
      aAfterParams.get('sort') === 'Name:Asc' &&
      aAfterParams.get('hidden') === '1' &&
      aAfter.href === aBefore.href &&
      aAfter.len === aBefore.len &&
      aAfter.cb === aBefore.cb &&
      aAfter.prefs.view === 'grid' &&
      aAfter.prefs.sort === 'Size:Desc' &&
      aAfter.prefs.hidden === '0' &&
      tabA.pageErrors.length === 0 &&
      tabB.pageErrors.length === 0;
    rec(
      '12 prefs isolation: tab A reload restores own tree+hidden snapshot despite other tab prefs',
      ok,
      ' aBefore=' + JSON.stringify(aBefore) + ' bAfter=' + JSON.stringify(bAfter) +
        ' shared=' + JSON.stringify(shared) + ' aAfter=' + JSON.stringify(aAfter) +
        ' errors=' + JSON.stringify(tabA.pageErrors.concat(tabB.pageErrors))
    );
  } finally {
    try {
      await tabB.page.close();
    } catch {
      /* 目标已销毁 */
    }
  }
});

await runScenario('13 hidden PDF-suffixed directory: preview survives hiding and reload without native PDF open', async () => {
  const { page, pageErrors } = await fresh('dir');
  await page.click('[data-cb-hidden-toggle]');
  await waitFor(
    () => page.evaluate("!!document.querySelector('.cb-row[data-native-path-encoded=\".hidden.pdf\"]')"),
    'hidden directory shown'
  );
  await page.click('.cb-row[data-native-path-encoded=".hidden.pdf"] [data-cb-action="preview"]');
  await waitFor(
    () => page.evaluate("!!document.querySelector('#cb-preview-content .cb-dir-fragment [data-native-path-encoded=\".hidden.pdf/inside.md\"]')"),
    'hidden directory preview'
  );
  const len = await page.evaluate('history.length');
  await page.click('[data-cb-hidden-toggle]');
  await waitFor(
    () => page.evaluate("document.body.dataset.cbHidden === '0' && !document.querySelector('.cb-row[data-native-path-encoded=\".hidden.pdf\"]')"),
    'hidden directory filtered'
  );
  const before = await page.evaluate(`(() => ({
    path: location.pathname, len: history.length, hidden: history.state.cb.hidden,
    opened: history.state.cb.opened, kind: history.state.cb.openedKind,
    panel: !document.querySelector('#cb-preview').hidden, calls: window.__openSpy.length
  }))()`);
  await page.evaluate('window.__hiddenDirReload = 1');
  await page.send('Page.reload');
  await waitFor(
    () => page.evaluate("typeof window.__hiddenDirReload === 'undefined' && !!document.querySelector('#cb-preview-content .cb-dir-fragment [data-native-path-encoded=\".hidden.pdf/inside.md\"]')"),
    'hidden directory restored',
    12000
  );
  const after = await page.evaluate(`(() => ({
    path: location.pathname, len: history.length, hidden: history.state.cb.hidden,
    opened: history.state.cb.opened, kind: history.state.cb.openedKind,
    panel: !document.querySelector('#cb-preview').hidden, calls: window.__openSpy.length
  }))()`);
  rec(
    '13 hidden PDF-suffixed directory: preview survives hiding and reload without native PDF open',
    before.path === '/' && before.len === len && before.hidden === 0 &&
      before.opened === '.hidden.pdf' && before.kind === 'directory' && before.panel &&
      before.calls === 0 && JSON.stringify(after) === JSON.stringify(before) && pageErrors.length === 0,
    JSON.stringify({before,after,errors:pageErrors})
  );
});

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
