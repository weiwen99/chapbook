// 模式切换按钮移入左侧工具栏验收 — 确定性 CDP 浏览器验收.
//
// 手动调用 (非 cargo test), 自包含: 仅 Node 内置模块 + 系统 Chrome 的 CDP
// WebSocket; 自建临时 fixture, 自行启动 target/release/chapbook 并监听固定
// 测试端口, finally 清理 server/chrome/fixture; 任一断言失败以非零退出.
//
// 运行:
//   cargo build --release && node tests/browser_mode_toggle_toolbar_acceptance.mjs
//
// Chrome 定位: $CHROME_BIN 优先, 其次 ms-playwright / Chrome-for-Testing 已知
// 路径 (macOS 与 Linux) 与系统常见位置.
//
// 场景:
//   M1  目录页目录模式: body 顶部左缘工具栏行含迁入的顶栏搜索表单 + 模式切换
//       按钮 (末位); 无浮动 fixed/top/right 定位; 标签/aria-pressed 正确
//   M2  切换进树模式 (宽屏): 目录工具栏行隐藏; 树顶栏顺序 搜索→折叠→模式切换;
//       按钮同元素跨模式移动; localStorage 持久化; 无浮动定位; 无页面错误
//   M3  切回目录模式: 工具栏行恢复; 按钮迁回; 键清除 (搜索框值随整页重载
//       重置, 非本变更契约)
//   M4  持久化: 树模式下整页重载仍为树模式 (localStorage 生效)
//   M5  窄屏抽屉: 抽屉顶栏含搜索 + 模式切换, 折叠按钮隐藏; 抽屉内切换可用
//   M6  搜索页: 无工具栏行/无切换按钮/无树壳; 顶栏搜索表单保持原位
//   M7  文档页: 左缘工具栏行含树结构搜索表单 (独立 id) + 切换按钮; 树模式
//       TOC 列几何不变 (240px); 分隔线几何不变
//   M8  折叠/恢复: 折叠后恢复按钮可见、树隐藏; 恢复后展开; 偏好持久化
//   M9  调整器拖拽: 指针拖拽改树宽 + 持久化; 网格列几何 (238px 8px 1fr)

import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
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
        /* 导航中 */
      }
      await sleep(50);
    }
    throw new Error('waitLoad timeout');
  }

  // 应用侧全部经 document 级 click 监听分发, 无 isTrusted 依赖 — 合成 DOM
  // click 与真实指针事件走同一处理器, 确定性最高.
  async click(selector) {
    const ok = await this.evaluate(
      '(() => { const el = document.querySelector(' + JSON.stringify(selector) + '); if (!el) return false; el.click(); return true; })()'
    );
    if (!ok) throw new Error('click: no element for ' + selector);
  }

  async close() {
    try {
      await this.send('Fetch.disable');
    } catch {
      /* fetch 域未启用 */
    }
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
  const root = mkdtempSync(join(tmpdir(), 'cb-mode-accept-fixture-'));
  writeFileSync(join(root, 'a.md'), '# A\n\npage a.\n\n## A-2\n\nmore.\n');
  mkdirSync(join(root, 'adir', 'grand'), { recursive: true });
  writeFileSync(join(root, 'adir', 'child.md'), '# Child\n\nchild page.\n');
  writeFileSync(join(root, 'adir', 'grand', 'leaf.md'), '# Leaf\n\nleaf page.\n');
  mkdirSync(join(root, 'emptydir'));
  symlinkSync('..', join(root, 'adir', 'grand', 'loop'));
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
  for (const port of [7887, 7888, 7889]) {
    if (await probePort(port)) return port;
  }
  throw new Error('no free fixed test port (7887-7889)');
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
chromeProfile = mkdtempSync(join(tmpdir(), 'cb-mode-accept-chrome-'));
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
    '--disable-features=BackForwardCache',
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

let activePage = null;

async function runScenario(name, fn) {
  try {
    await fn();
  } finally {
    if (activePage) {
      await activePage.close();
      activePage = null;
    }
  }
}

async function fresh(width = 1280, height = 800) {
  const { targetId } = await browserCdp.send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await browserCdp.send('Target.attachToTarget', {
    targetId,
    flatten: true,
  });
  const page = new Page(browserCdp, targetId, sessionId);
  activePage = page;
  const pageErrors = [];
  const onException = (params, sid) => {
    if (sid !== sessionId) return;
    const d = params.exceptionDetails;
    pageErrors.push('pageerror: ' + (d.exception ? d.exception.description : d.text));
  };
  page.on('Runtime.exceptionThrown', onException);
  await page.send('Runtime.enable');
  await page.send('Page.enable');
  await page.send('Emulation.setDeviceMetricsOverride', {
    width,
    height,
    deviceScaleFactor: 0,
    mobile: false,
  });
  await page.send('Storage.clearDataForOrigin', {
    origin: BASE,
    storageTypes: 'local_storage',
  });
  await page.goto(BASE + '/');
  return { page, pageErrors };
}

// 切换进树模式并等待根列表落地
async function enterTree(page, waitMs = 1500) {
  if (!(await page.evaluate("document.body.classList.contains('cb-tree-mode')"))) {
    await page.click('.cb-mode-toggle');
  }
  const t0 = Date.now();
  while (Date.now() - t0 < 10000) {
    if (await page.evaluate("!!document.querySelector('ul#cb-tree-root > li.cb-tree-node')")) break;
    await sleep(100);
  }
  await sleep(waitMs);
}

/* =====================================================================
 * M1 目录页目录模式: body 顶部左缘工具栏行 (搜索表单迁入 + 模式切换末位),
 *     无浮动 fixed/top/right 定位, 标签/aria-pressed 正确.
 * ===================================================================== */
await runScenario('M1 directory mode: left toolbar row with moved topbar search + trailing mode toggle, no floating', async () => {
  const { page, pageErrors } = await fresh();
  const s = await page.evaluate(
    '(() => { const tb = document.querySelector(".cb-mode-toolbar"); const btn = document.querySelector(".cb-mode-toggle"); const topbar = document.querySelector("header.cb-topbar"); const cs = btn ? getComputedStyle(btn) : null; const cb = btn ? btn.getBoundingClientRect() : null; return { toolbarExists: !!tb, isFirstBodyChild: tb === document.body.firstElementChild, searchInToolbar: !!tb && !!tb.querySelector("form.cb-search #cb-search-q"), searchStillInTopbar: !!topbar && !!topbar.querySelector("form.cb-search"), toggleInToolbar: !!tb && !!tb.querySelector(".cb-mode-toggle"), toggleIsLastChild: !!tb && tb.lastElementChild.className === "cb-mode-toggle", toggleCount: document.querySelectorAll(".cb-mode-toggle").length, directBodyChild: btn ? btn.parentElement === document.body : null, pos: cs ? cs.position : null, top: cs ? cs.top : null, right: cs ? cs.right : null, radius: cs ? cs.borderRadius : null, height: cs ? cs.height : null, label: btn ? btn.textContent : null, pressed: btn ? btn.getAttribute("aria-pressed") : null, title: btn ? btn.title : null, rectRightGap: cb ? Math.round(window.innerWidth - cb.right) : null }; })()'
  );
  rec('M1 directory mode: left toolbar row with moved topbar search + trailing mode toggle, no floating',
    s.toolbarExists && s.isFirstBodyChild && s.searchInToolbar && !s.searchStillInTopbar &&
      s.toggleInToolbar && s.toggleIsLastChild && s.toggleCount === 1 && s.directBodyChild === false &&
      s.pos === 'static' && s.top === 'auto' && s.right === 'auto' && s.radius === '0px' &&
      parseFloat(s.height) >= 25 && parseFloat(s.height) <= 26 && s.label === '树' && s.pressed === 'false' &&
      s.title === '切换到树模式' && pageErrors.length === 0,
    JSON.stringify(s) + ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * M2 切换进树模式 (宽屏): 目录工具栏行隐藏; 树顶栏顺序 搜索→折叠→模式切换;
 *     按钮同元素跨模式移动; localStorage 持久化; 无浮动定位; 无页面错误.
 * ===================================================================== */
await runScenario('M2 tree mode: toolbar row hidden, tree toolbar order search/collapse/toggle, same element moved, persisted', async () => {
  const { page, pageErrors } = await fresh();
  // 记录目录行内按钮元素身份 (跨模式移动必须同一节点)
  const btnId = await page.evaluate(
    '(() => { const b = document.querySelector(".cb-mode-toggle"); if (b) b.dataset.marker = "same-btn"; return !!b; })()'
  );
  await enterTree(page);
  const s = await page.evaluate(
    '(() => { const tb = document.querySelector(".cb-mode-toolbar"); const toolbar = document.querySelector(".cb-tree-toolbar"); const btn = document.querySelector(".cb-mode-toggle"); const cs = btn ? getComputedStyle(btn) : null; const kids = toolbar ? [...toolbar.children].map((el) => el.className) : null; return { treeMode: document.body.classList.contains("cb-tree-mode"), toolbarHidden: tb ? tb.hidden : null, toolbarDisplay: tb ? getComputedStyle(tb).display : null, toolbarKids: kids, sameBtn: btn ? btn.dataset.marker === "same-btn" : false, btnInTreeToolbar: btn ? btn.closest(".cb-tree-toolbar") !== null : false, label: btn ? btn.textContent : null, pressed: btn ? btn.getAttribute("aria-pressed") : null, stored: localStorage.getItem("cb-mode"), pos: cs ? cs.position : null, navFirstChildToolbar: (() => { const nav = document.getElementById("cb-tree-nav"); return nav ? nav.firstElementChild.className === "cb-tree-toolbar" : false; })() }; })()'
  );
  rec('M2 tree mode: toolbar row hidden, tree toolbar order search/collapse/toggle, same element moved, persisted',
    s.treeMode && s.toolbarHidden === true && s.toolbarDisplay === 'none' &&
      JSON.stringify(s.toolbarKids) === JSON.stringify(['cb-tree-search', 'cb-tree-collapse', 'cb-mode-toggle']) &&
      s.sameBtn && s.btnInTreeToolbar && s.label === '目录' && s.pressed === 'true' &&
      s.stored === 'tree' && s.pos === 'static' && s.navFirstChildToolbar &&
      pageErrors.length === 0,
    JSON.stringify(s) + ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * M3 切回目录模式: 工具栏行恢复; 按钮迁回; 搜索框值跨模式保留; 键清除.
 * ===================================================================== */
await runScenario('M3 exit tree: toolbar row restored, button back, mode key cleared', async () => {
  const { page, pageErrors } = await fresh();
  // 目录页退出树模式走整页重载 (pane SPA 恢复完整页, 既有行为): 退出后必须
  // 回到目录模式 — 左缘工具栏行恢复、切换按钮在行内、键清除. 搜索框值随
  // 重载重置 (非本变更契约).
  await page.click('.cb-mode-toggle'); // 进树模式
  const t0 = Date.now();
  while (Date.now() - t0 < 10000) {
    if (await page.evaluate("!!document.querySelector('ul#cb-tree-root > li.cb-tree-node')")) break;
    await sleep(100);
  }
  await sleep(300);
  await page.click('.cb-mode-toggle'); // 退出
  const t1 = Date.now();
  while (Date.now() - t1 < 10000) {
    if (await page.evaluate("!document.body.classList.contains('cb-tree-mode')")) break;
    await sleep(100);
  }
  await sleep(300);
  const s = await page.evaluate(
    '(() => { const tb = document.querySelector(".cb-mode-toolbar"); const btn = document.querySelector(".cb-mode-toggle"); return { treeMode: document.body.classList.contains("cb-tree-mode"), toolbarVisible: tb ? !tb.hidden && getComputedStyle(tb).display !== "none" : false, btnInToolbar: btn ? btn.closest(".cb-mode-toolbar") !== null : false, searchInToolbar: tb ? !!tb.querySelector("form.cb-search #cb-search-q") : false, label: btn ? btn.textContent : null, stored: localStorage.getItem("cb-mode"), toggleInTree: !!document.querySelector(".cb-tree-toolbar .cb-mode-toggle"), count: document.querySelectorAll(".cb-mode-toggle").length }; })()'
  );
  rec('M3 exit tree: toolbar row restored, button back, mode key cleared',
    !s.treeMode && s.toolbarVisible && s.btnInToolbar && s.searchInToolbar &&
      s.label === '树' && s.stored === null && !s.toggleInTree && s.count === 1 && pageErrors.length === 0,
    JSON.stringify(s) + ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * M4 持久化: 树模式下整页重载仍为树模式 (localStorage 生效).
 * ===================================================================== */
await runScenario('M4 persistence: full reload in tree mode stays tree mode', async () => {
  const { page, pageErrors } = await fresh();
  await enterTree(page);
  await page.goto(BASE + '/'); // 同 origin 重载: localStorage 保留
  await sleep(800);
  const s = await page.evaluate(
    '(() => { const btn = document.querySelector(".cb-mode-toggle"); return { treeMode: document.body.classList.contains("cb-tree-mode"), toolbarHidden: (() => { const tb = document.querySelector(".cb-mode-toolbar"); return tb ? tb.hidden : null; })(), btnInTreeToolbar: btn ? btn.closest(".cb-tree-toolbar") !== null : false, label: btn ? btn.textContent : null, pressed: btn ? btn.getAttribute("aria-pressed") : null }; })()'
  );
  rec('M4 persistence: full reload in tree mode stays tree mode',
    s.treeMode && s.toolbarHidden === true && s.btnInTreeToolbar && s.label === '目录' && s.pressed === 'true' && pageErrors.length === 0,
    JSON.stringify(s) + ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * M5 窄屏抽屉: 抽屉顶栏含搜索 + 模式切换, 折叠按钮隐藏; 抽屉内切换可用.
 * ===================================================================== */
await runScenario('M5 narrow drawer: drawer toolbar has search + toggle, collapse hidden, toggle works', async () => {
  const { page, pageErrors } = await fresh(700, 800);
  await enterTree(page, 600);
  const before = await page.evaluate(
    '(() => { const ob = document.querySelector(".cb-tree-open"); return { openBtnVisible: ob ? getComputedStyle(ob).display !== "none" : false, collapseVisible: (() => { const c = document.querySelector(".cb-tree-collapse"); return c ? getComputedStyle(c).display !== "none" : null; })() }; })()'
  );
  await page.click('.cb-tree-open'); // 打开抽屉
  await sleep(400);
  const open = await page.evaluate(
    '(() => { const drawer = document.getElementById("cb-tree-nav"); const toolbar = document.querySelector(".cb-tree-toolbar"); const kids = toolbar ? [...toolbar.children].map((el) => el.className) : null; const collapse = document.querySelector(".cb-tree-collapse"); const toggle = document.querySelector(".cb-mode-toggle"); return { drawerOpen: document.body.classList.contains("cb-drawer-open"), kids, collapseDisplay: collapse ? getComputedStyle(collapse).display : null, toggleInToolbar: toggle ? toggle.closest(".cb-tree-toolbar") !== null : false, toggleVisible: toggle ? getComputedStyle(toggle).display !== "none" : false }; })()'
  );
  // 抽屉内切换: 点击模式切换 → 退出树模式, 目录工具栏行恢复
  await page.click('.cb-mode-toggle');
  const t0 = Date.now();
  while (Date.now() - t0 < 10000) {
    if (await page.evaluate("!document.body.classList.contains('cb-tree-mode')")) break;
    await sleep(100);
  }
  await sleep(300);
  const after = await page.evaluate(
    '(() => { const tb = document.querySelector(".cb-mode-toolbar"); const btn = document.querySelector(".cb-mode-toggle"); return { treeMode: document.body.classList.contains("cb-tree-mode"), toolbarVisible: tb ? !tb.hidden && getComputedStyle(tb).display !== "none" : false, btnInToolbar: btn ? btn.closest(".cb-mode-toolbar") !== null : false, label: btn ? btn.textContent : null }; })()'
  );
  rec('M5 narrow drawer: drawer toolbar has search + toggle, collapse hidden, toggle works',
    before.openBtnVisible && before.collapseVisible === false &&
      open.drawerOpen && JSON.stringify(open.kids) === JSON.stringify(['cb-tree-search', 'cb-tree-collapse', 'cb-mode-toggle']) &&
      open.collapseDisplay === 'none' && open.toggleInToolbar && open.toggleVisible &&
      !after.treeMode && after.toolbarVisible && after.btnInToolbar && after.label === '树' &&
      pageErrors.length === 0,
    'before=' + JSON.stringify(before) + ' open=' + JSON.stringify(open) + ' after=' + JSON.stringify(after) +
      ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * M6 搜索页: 无工具栏行/无切换按钮/无树壳; 顶栏搜索表单保持原位.
 * ===================================================================== */
await runScenario('M6 search page: no toolbar row, no toggle, no tree shell; topbar search untouched', async () => {
  const { page, pageErrors } = await fresh();
  await page.goto(BASE + '/__/search?q=leaf');
  await sleep(500);
  const s = await page.evaluate(
    '(() => { const topbar = document.querySelector("header.cb-topbar"); return { toolbar: !!document.querySelector(".cb-mode-toolbar"), toggle: !!document.querySelector(".cb-mode-toggle"), tree: !!document.getElementById("cb-tree-nav"), treeMode: document.body.classList.contains("cb-tree-mode"), topbarSearch: !!topbar && !!topbar.querySelector("form.cb-search #cb-search-q"), hasResults: !!document.querySelector("main[data-cb-parent] .cb-view-list") }; })()'
  );
  rec('M6 search page: no toolbar row, no toggle, no tree shell; topbar search untouched',
    !s.toolbar && !s.toggle && !s.tree && !s.treeMode && s.topbarSearch && s.hasResults && pageErrors.length === 0,
    JSON.stringify(s) + ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * M7 文档页: 左缘工具栏行含树结构搜索表单 (独立 id) + 切换按钮; 树模式
 *     TOC 列几何不变 (240px); 分隔线几何不变.
 * ===================================================================== */
await runScenario('M7 doc page: toolbar row with tree-structure search (own id) + toggle; TOC/separator geometry intact', async () => {
  const { page, pageErrors } = await fresh(1280, 800);
  await page.goto(BASE + '/a.md');
  await sleep(500);
  const dir = await page.evaluate(
    '(() => { const tb = document.querySelector(".cb-mode-toolbar"); const btn = document.querySelector(".cb-mode-toggle"); return { toolbarExists: !!tb, treeSearchForm: !!tb && !!tb.querySelector("form.cb-tree-search #cb-mode-search-q"), toggleLast: !!tb && tb.lastElementChild.className === "cb-mode-toggle", label: btn ? btn.textContent : null, topbarSearch: !!document.querySelector("header.cb-topbar form.cb-search") }; })()'
  );
  await enterTree(page, 800);
  const tree = await page.evaluate(
    '(() => { const layout = document.querySelector(".cb-pane .cb-doc-layout"); const toc = document.querySelector(".cb-pane .cb-doc-toc"); const resizer = document.querySelector(".cb-tree-resizer"); const grid = getComputedStyle(document.body).gridTemplateColumns; const resizerBefore = resizer ? getComputedStyle(resizer, "::before").width : null; return { grid, resizerWidth: resizer ? resizer.getBoundingClientRect().width : null, resizerBefore, layoutColumns: layout ? getComputedStyle(layout).gridTemplateColumns : null, tocExists: !!toc, tocWidth: toc ? Math.round(toc.getBoundingClientRect().width) : null }; })()'
  );
  rec('M7 doc page: toolbar row with tree-structure search (own id) + toggle; TOC/separator geometry intact',
    dir.toolbarExists && dir.treeSearchForm && dir.toggleLast && dir.label === '树' && !dir.topbarSearch &&
      tree.grid.startsWith('238px 8px ') && tree.resizerWidth === 8 && tree.resizerBefore === '2px' &&
      tree.layoutColumns && tree.layoutColumns.endsWith('240px') && tree.tocExists && tree.tocWidth === 240 &&
      pageErrors.length === 0,
    'dir=' + JSON.stringify(dir) + ' tree=' + JSON.stringify(tree) + ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * M8 折叠/恢复: 折叠后恢复按钮可见、树隐藏; 恢复后展开; 偏好持久化.
 * ===================================================================== */
await runScenario('M8 collapse/restore: collapse hides tree + shows restore, restore expands, preference persisted', async () => {
  const { page, pageErrors } = await fresh();
  await enterTree(page, 600);
  await page.click('.cb-tree-collapse');
  await sleep(400);
  const collapsed = await page.evaluate(
    '(() => { const restore = document.querySelector(".cb-tree-restore"); const tree = document.getElementById("cb-tree-nav"); return { cls: document.body.classList.contains("cb-tree-collapsed"), restoreVisible: restore ? !restore.hidden && getComputedStyle(restore).display !== "none" : false, treeDisplay: tree ? getComputedStyle(tree).display : null, stored: localStorage.getItem("cb-tree-collapsed"), toolbarToggleStillThere: !!document.querySelector(".cb-mode-toggle") }; })()'
  );
  await page.click('.cb-tree-restore');
  await sleep(400);
  const restored = await page.evaluate(
    '(() => { const collapse = document.querySelector(".cb-tree-collapse"); return { cls: document.body.classList.contains("cb-tree-collapsed"), collapseVisible: collapse ? getComputedStyle(collapse).display !== "none" : false, treeVisible: (() => { const t = document.getElementById("cb-tree-nav"); return t ? getComputedStyle(t).display !== "none" : false; })() }; })()'
  );
  rec('M8 collapse/restore: collapse hides tree + shows restore, restore expands, preference persisted',
    collapsed.cls && collapsed.restoreVisible && collapsed.treeDisplay === 'none' &&
      collapsed.stored === '1' && collapsed.toolbarToggleStillThere &&
      !restored.cls && restored.collapseVisible && restored.treeVisible && pageErrors.length === 0,
    'collapsed=' + JSON.stringify(collapsed) + ' restored=' + JSON.stringify(restored) + ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * M9 调整器拖拽: 指针拖拽改树宽 + 持久化; 网格列几何 (238px 8px 1fr).
 * ===================================================================== */
await runScenario('M9 resizer drag: pointer drag resizes tree + persists; grid geometry 238px 8px 1fr', async () => {
  const { page, pageErrors } = await fresh();
  await enterTree(page, 600);
  const gridBefore = await page.evaluate("getComputedStyle(document.body).gridTemplateColumns");
  const rect = await page.evaluate(
    '(() => { const r = document.querySelector(".cb-tree-resizer").getBoundingClientRect(); return { x: Math.round(r.x + r.width / 2), y: Math.round(r.y + r.height / 2) }; })()'
  );
  await page.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: rect.x, y: rect.y, button: 'left', clickCount: 1 });
  await page.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: rect.x + 60, y: rect.y, button: 'left', buttons: 1 });
  await page.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: rect.x + 60, y: rect.y, button: 'left', clickCount: 1 });
  await sleep(400);
  const s = await page.evaluate(
    '(() => { const grid = getComputedStyle(document.body).gridTemplateColumns; return { grid, widthVar: document.body.style.getPropertyValue("--cb-tree-width"), stored: localStorage.getItem("cb-tree-width"), resizing: document.body.classList.contains("cb-tree-resizing") }; })()'
  );
  const w = parseInt(s.widthVar, 10);
  rec('M9 resizer drag: pointer drag resizes tree + persists; grid geometry 238px 8px 1fr',
    gridBefore.startsWith('238px 8px ') && w >= 290 && w <= 310 && s.stored === String(w) &&
      !s.resizing && pageErrors.length === 0,
    'gridBefore=' + gridBefore + ' after=' + JSON.stringify(s) + ' errors=' + JSON.stringify(pageErrors));
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
