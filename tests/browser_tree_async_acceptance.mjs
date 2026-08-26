// Task5 异步树竞态验收 — 确定性 CDP 浏览器验收 (A/A2/B/C/D/E/F/G + nav-only stale cleanup).
//
// 手动调用 (非 cargo test), 自包含: 仅 Node 内置模块 + 系统 Chrome 的 CDP
// WebSocket (无任何 npm 包); 自建临时 fixture (目录/文件/空目录/循环符号链接),
// 自行启动 target/release/chapbook 并监听固定测试端口, finally 清理
// server/chrome/fixture; 任一断言失败以非零退出.
//
// 运行:
//   cargo build --release && node tests/browser_tree_async_acceptance.mjs
//
// Chrome 定位: $CHROME_BIN 优先, 其次 ms-playwright / Chrome-for-Testing 已知
// 路径 (macOS 与 Linux, 完整浏览器优先、同类型取最新版本) 与系统常见位置.
//
// 场景:
//   A   陈旧 nav owner 响应 + 当前壳首次点击续接: 恰一次请求, 首击即展开
//   A2  在途两次点击 (显式翻转): 恰一次请求, 落地折叠, 第三次点击缓存命中展开
//   B   nav 续接方提交前更新导航: 陈旧响应零 DOM/持久化/current/选中变更
//   C   旧/新壳同路径 (abort no-op): 旧壳响应不得删除新壳在途/意图, 新壳安装一次
//   D   已缓存折叠祖先 → 面板下钻子项: 祖先缓存命中展开, 子项 current/选中
//   E   空目录持久化展开重复条目: 叶子化移除全部重复, 零披露零额外请求
//   F   根加载失败: nav 内 role=alert 文本错误, 零未处理拒绝; 重进新壳恢复
//   G   行点击焦点落树根 (preventScroll) + 键盘; ARIA 可访问名排除字形
//   N   陈旧 nav-only 成功落地无续接方: 缓存存在/在途清除/默认 desired 不残留,
//       后续当前导航缓存命中展开, 零额外请求

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
  // 完整浏览器优先, 同类型取最新版本 (旧缓存可能是残缺 stub 安装)
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
  // isTrusted 依赖 — 合成 DOM click 与真实指针事件走同一处理器, 且不受坐标
  // 竞态 (元素移动/重绘) 与双击连击 (detail>1) 影响, 确定性最高.
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
  const root = mkdtempSync(join(tmpdir(), 'cb-tree-accept-fixture-'));
  writeFileSync(join(root, 'a.md'), '# A\n\npage a.\n');
  mkdirSync(join(root, 'adir', 'grand'), { recursive: true });
  writeFileSync(join(root, 'adir', 'child.md'), '# Child\n\nchild page.\n');
  writeFileSync(join(root, 'adir', 'grand', 'leaf.md'), '# Leaf\n\nleaf page.\n');
  mkdirSync(join(root, 'bdir'));
  writeFileSync(join(root, 'bdir', 'x.md'), '# X\n\nx page.\n');
  mkdirSync(join(root, 'emptydir'));
  // 循环符号链接: grand/loop -> .. (即 adir, 已访问目录) — 目录身份去重终止,
  // 服务端不无限递归; 仅证明 fixture 含环, 不参与任何断言计数.
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
  for (const port of [7897, 7898, 7899]) {
    if (await probePort(port)) return port;
  }
  throw new Error('no free fixed test port (7897-7899)');
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
    rmSync(chromeProfile, { recursive: true, force: true });
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
chromeProfile = mkdtempSync(join(tmpdir(), 'cb-tree-accept-chrome-'));
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
    // 禁用 bfcache: 场景间同 URL 导航必须产生全新文档与全新 init —
    // 上一场景的持久化状态若被 bfcache 恢复的旧文档内存态继承, 进入链
    // 持久化 walk 会预加载目录并污染场景请求计数.
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

const pausedCounts = {};
const netCounts = {};

async function fresh(mode, opts) {
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
  const onRequest = (params, sid) => {
    if (sid !== sessionId) return;
    const m = params.request.url.match(/\/__\/tree(\/.*)?$/);
    if (m) {
      const p = m[1] || '/';
      netCounts[p] = (netCounts[p] || 0) + 1;
    }
  };
  page.on('Runtime.exceptionThrown', onException);
  page.on('Network.requestWillBeSent', onRequest);
  await page.send('Runtime.enable');
  await page.send('Page.enable');
  await page.send('Network.enable');
  // 固定宽屏视口: 默认 headless 窗口可能 <64rem 触发窄屏抽屉 (树 off-canvas),
  // 行坐标落在视口外, 点击必然落空 — 与 Playwright 版验收的 1280x800 一致.
  await page.send('Emulation.setDeviceMetricsOverride', {
    width: 1280,
    height: 800,
    deviceScaleFactor: 0,
    mobile: false,
  });

  // 进入页面前先清空 origin 的 localStorage: 场景间同 origin 共享存储, 上一
  // 场景写入的 cb-tree-expanded/cb-tree-selected 若被进入链持久化 walk 读取,
  // 会预加载目录并污染场景请求计数 — 首个 goto 即全新文档全新 init, 绝不依赖
  // clear+reload 的时序.
  await page.send('Storage.clearDataForOrigin', {
    origin: BASE,
    storageTypes: 'local_storage',
  });
  await page.goto(BASE + '/');
  if (opts && opts.nohost) {
    await page.evaluate(
      "(() => { document.querySelector('main.cb-main')?.remove(); document.querySelector('.cb-doc')?.remove(); })()"
    );
  }
  if (opts && opts.patchAbort) {
    // 传输层中止伪装 no-op (计数): 跨壳场景只测所有权防御 (gen 守卫/身份比对),
    // 不测传输取消 — 旧壳响应必须真实 resolve 才能验证守卫.
    await page.evaluate(
      "(() => { window.__abortMock = 0; const OrigAC = AbortController; window.AbortController = function () { const ac = new OrigAC(); ac.abort = function () { window.__abortMock++; }; return ac; }; })()"
    );
  }
  if (mode === 'tree') {
    if (!(await page.evaluate("document.body.classList.contains('cb-tree-mode')"))) {
      await page.click('.cb-mode-toggle');
    }
    await waitFor(
      () => page.evaluate("!!document.querySelector('ul#cb-tree-root > li.cb-tree-node')"),
      'root attached',
      10000
    );
    await sleep(400); // 根 JSON settle
  }
  // 每场景独立计数 (模块级表不得跨场景泄漏)
  Object.keys(pausedCounts).forEach((k) => delete pausedCounts[k]);
  Object.keys(netCounts).forEach((k) => delete netCounts[k]);
  return { page, pageErrors };
}

// holdTreePath: 暂停 holdSet 内路径的全部 tree 请求 (held: path → [requestId]);
// failFirstRoot 非空时, 首次根请求直接 fulfill 500 (F 场景). 其余请求放行.
function holdTreePath(page, holdSet, held, failFirstRoot) {
  const handler = (params, sid) => {
    if (sid !== page.sessionId) return;
    const m = params.request.url.match(/\/__\/tree(\/.*)?$/);
    const p = m ? (m[1] || '/') : null;
    if (p === null) {
      page.send('Fetch.continueRequest', { requestId: params.requestId }).catch(() => {});
      return;
    }
    pausedCounts[p] = (pausedCounts[p] || 0) + 1;
    if (failFirstRoot && p === '/' && !failFirstRoot.done) {
      failFirstRoot.done = true;
      page.send('Fetch.fulfillRequest', {
        requestId: params.requestId,
        responseCode: 500,
        responseHeaders: [{ name: 'Content-Type', value: 'application/json' }],
        body: Buffer.from('{"error":"boom"}').toString('base64'),
      }).catch(() => {});
      return;
    }
    if (holdSet.has(p)) {
      const list = held.get(p) || [];
      list.push(params.requestId);
      held.set(p, list);
    } else {
      page.send('Fetch.continueRequest', { requestId: params.requestId }).catch(() => {});
    }
  };
  page.on('Fetch.requestPaused', handler);
  return page.send('Fetch.enable', {
    patterns: [{ urlPattern: '*__/tree*' }],
    requestStage: 'Request',
  });
}

async function releaseOne(page, held, path) {
  const list = held.get(path);
  if (list && list.length) {
    const id = list.shift();
    if (list.length === 0) held.delete(path);
    await page.send('Fetch.continueRequest', { requestId: id }).catch((e) => {
      throw new Error('release failed: ' + e.message);
    });
    return id;
  }
  return null;
}

async function waitHeld(page, held, path, n, label) {
  const t0 = Date.now();
  while (Date.now() - t0 < 8000) {
    const list = held.get(path) || [];
    if (list.length >= n) return true;
    await sleep(50);
  }
  throw new Error('waitHeld timeout: ' + label + ' (want ' + n + ', got ' + ((held.get(path) || []).length) + ')');
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

const dirState = (page, path) =>
  page.evaluate(
    '(() => { const li = document.querySelector(' + JSON.stringify('[data-native-path-encoded="' + path + '"]') + '); const ul = li ? li.querySelector(":scope > ul.cb-tree-children") : null; return { expandable: li ? li.dataset.treeExpandable : null, expanded: li ? li.getAttribute("aria-expanded") : null, twist: li ? li.querySelector(".cb-tree-twist").textContent : null, hasUl: !!ul, hidden: ul ? ul.hidden : null, children: ul ? ul.querySelectorAll(":scope > li").length : -1 }; })()'
  );

const expandedList = (page) =>
  page.evaluate("JSON.parse(localStorage.getItem('cb-tree-expanded') || 'null')");

const currentPaths = (page) =>
  page.evaluate(
    "Array.from(document.querySelectorAll('#cb-tree-root .cb-current')).map((el) => el.dataset.nativePathEncoded)"
  );

const selState = (page) =>
  page.evaluate(
    '(() => { const root = document.getElementById("cb-tree-root"); const ad = root ? root.getAttribute("aria-activedescendant") : null; const sel = document.querySelector("#cb-tree-root .cb-tree-node.cb-selected"); return { selected: sel ? sel.dataset.nativePathEncoded : null, ad, adLive: ad ? !!document.getElementById(ad) : false, selTrue: document.querySelectorAll("#cb-tree-root [aria-selected=\\"true\\"]").length }; })()'
  );

async function runScenario(name, fn) {
  try {
    await fn();
  } catch (e) {
    rec(name, false, 'THREW: ' + String(e && e.message ? e.message : e));
  }
}

/* =====================================================================
 * A. 陈旧 nav owner P (导航已提交 B) + 当前壳首次点击续接: desired true +
 *    显式标记 + ownerless 续接方; release 后恰一次请求, 首击可见展开; 指针
 *    焦点落树根. (旧 bug: 首击翻转 nav 隐藏 desired 并裸返回无续接 → 陈旧
 *    响应不安装 → 需要第三次点击.)
 * ===================================================================== */
await runScenario('A stale nav owner: first click while pending -> desired true + adopter; release exactly one request, visibly expanded', async () => {
  const { page, pageErrors } = await fresh('tree');
  const holdSet = new Set(['/adir']);
  const held = new Map();
  await holdTreePath(page, holdSet, held);

  // nav A: 面板迷你列表 -> /adir -> revealPath 加载 adir (held, nav 默认 desired=true)
  await waitFor(() => page.evaluate("!!document.querySelector('.cb-pane .cb-dir-mini a[href=\"/adir\"]')"), 'pane-mini-ready');
  await page.click('.cb-pane .cb-dir-mini a[href="/adir"]');
  await waitHeld(page, held, '/adir', 1, 'A-adir-held');
  await sleep(300);
  // nav B: 树行点击根级文件 a.md -> 提交 (nav A 陈旧; adir 仍在途)
  await page.click('[data-native-path-encoded="a.md"] .cb-tree-row');
  await waitFor(() => page.evaluate("location.pathname === '/a.md'"), 'commit-B');
  await sleep(300);
  const preClick = await dirState(page, 'adir');
  // 用户首次点击可见折叠的在途目录
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await sleep(400);
  const focusState = await page.evaluate(
    '(() => ({ activeId: document.activeElement ? document.activeElement.id : null, activeTag: document.activeElement ? document.activeElement.tagName : null }))()'
  );
  const mid = { held: (held.get('/adir') || []).length, paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  // release: 恰一次请求; 续接方安装 -> 可见展开
  await releaseOne(page, held, '/adir');
  await waitFor(() => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"adir\"] ul.cb-tree-children li')"), 'adir-landed');
  await sleep(400);
  const final = await dirState(page, 'adir');
  const expanded = await expandedList(page);
  const cur = await currentPaths(page);
  const sel = await selState(page);
  const counts = { paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  rec('A stale nav owner: first click while pending -> desired true + adopter; release exactly one request, visibly expanded',
    preClick.hasUl === false && preClick.expanded === 'false' && preClick.twist === '▸' &&
      focusState.activeId === 'cb-tree-root' &&
      mid.held === 1 && mid.paused === 1 && mid.net === 1 &&
      final.children === 2 && final.hidden === false && final.expanded === 'true' && final.twist === '▾' &&
      Array.isArray(expanded) && expanded.includes('adir') &&
      cur.length === 1 && cur[0] === 'a.md' &&
      sel.selected === 'adir' && sel.adLive === true &&
      counts.paused === 1 && counts.net === 1 && pageErrors.length === 0,
    'pre=' + JSON.stringify(preClick) + ' focus=' + JSON.stringify(focusState) + ' mid=' + JSON.stringify(mid) +
      ' final=' + JSON.stringify(final) + ' expanded=' + JSON.stringify(expanded) + ' current=' + JSON.stringify(cur) +
      ' sel=' + JSON.stringify(sel) + ' counts=' + JSON.stringify(counts) + ' errors=' + JSON.stringify(pageErrors));
  await page.close();
});

/* =====================================================================
 * A2. 同 A 但用户在途期间点击两次 (显式翻转): 终态折叠; 仍恰一次请求;
 *     第三次点击缓存命中展开 — 每次点击都被兑现. (旧 bug: 点击 1 丢失,
 *     点击 2 用陈旧意图收起, 还需点击 3.)
 * ===================================================================== */
await runScenario('A2 two clicks while pending: exactly one request; final collapsed; third click expands from cache', async () => {
  const { page, pageErrors } = await fresh('tree');
  const holdSet = new Set(['/adir']);
  const held = new Map();
  await holdTreePath(page, holdSet, held);

  await waitFor(() => page.evaluate("!!document.querySelector('.cb-pane .cb-dir-mini a[href=\"/adir\"]')"), 'pane-mini-ready');
  await page.click('.cb-pane .cb-dir-mini a[href="/adir"]');
  await waitHeld(page, held, '/adir', 1, 'A2-adir-held');
  await sleep(300);
  await page.click('[data-native-path-encoded="a.md"] .cb-tree-row');
  await waitFor(() => page.evaluate("location.pathname === '/a.md'"), 'A2-commit-B');
  await sleep(300);
  // 在途期间点击两次: 点击1 = 显式展开意图, 点击2 = 翻转 -> 收起
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await sleep(250);
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await sleep(400);
  const mid = { held: (held.get('/adir') || []).length, paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  await releaseOne(page, held, '/adir');
  await waitFor(() => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"adir\"] ul.cb-tree-children li')"), 'A2-adir-landed');
  await sleep(400);
  const final = await dirState(page, 'adir');
  const expanded = await expandedList(page);
  const counts = { paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  // 第三次点击 (缓存命中, 在途已决): 展开 — 每次点击都被兑现
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await sleep(400);
  const third = await dirState(page, 'adir');
  rec('A2 two clicks while pending: exactly one request; final collapsed; third click expands from cache',
    mid.held === 1 && mid.paused === 1 && mid.net === 1 &&
      final.children === 2 && final.hidden === true && final.expanded === 'false' && final.twist === '▸' &&
      !(JSON.stringify(expanded) || '').includes('adir') &&
      third.hidden === false && third.expanded === 'true' && third.twist === '▾' && third.children === 2 &&
      counts.paused === 1 && counts.net === 1 && pageErrors.length === 0,
    'mid=' + JSON.stringify(mid) + ' final=' + JSON.stringify(final) + ' expanded=' + JSON.stringify(expanded) +
      ' third=' + JSON.stringify(third) + ' counts=' + JSON.stringify(counts) + ' errors=' + JSON.stringify(pageErrors));
  await page.close();
});

/* =====================================================================
 * B. nav 续接方开始时当前, release 前更新导航 -> 陈旧 nav 续接方响应零安装:
 *    无 DOM/无 localStorage/无 cb-current/无选中变更.
 * ===================================================================== */
await runScenario('B newer nav before release: stale nav adopter no DOM/persist/current/selection mutation', async () => {
  const { page, pageErrors } = await fresh('tree');
  const holdSet = new Set(['/adir']);
  const held = new Map();
  await holdTreePath(page, holdSet, held);

  await waitFor(() => page.evaluate("!!document.querySelector('.cb-pane .cb-dir-mini a[href=\"/adir\"]')"), 'pane-mini-ready');
  await page.click('.cb-pane .cb-dir-mini a[href="/adir"]');
  await waitHeld(page, held, '/adir', 1, 'B-adir-held');
  await sleep(300);
  await page.click('[data-native-path-encoded="a.md"] .cb-tree-row');
  await waitFor(() => page.evaluate("location.pathname === '/a.md'"), 'B-commit');
  await sleep(400);
  const s1 = {
    expanded: await expandedList(page),
    current: await currentPaths(page),
    sel: await selState(page),
  };
  await releaseOne(page, held, '/adir');
  await sleep(600);
  const s2 = {
    adir: await dirState(page, 'adir'),
    expanded: await expandedList(page),
    current: await currentPaths(page),
    sel: await selState(page),
    errBanner: await page.evaluate("!!document.querySelector('.cb-pane-error')"),
  };
  rec('B newer nav before release: stale nav adopter no DOM/persist/current/selection mutation',
    s2.adir.expanded === 'false' && s2.adir.hasUl === false && s2.adir.twist === '▸' &&
      JSON.stringify(s2.expanded) === JSON.stringify(s1.expanded) &&
      !(JSON.stringify(s2.expanded) || '').includes('adir') &&
      JSON.stringify(s2.current) === JSON.stringify(s1.current) && JSON.stringify(s2.current) === JSON.stringify(['a.md']) &&
      s2.sel.selected === 'a.md' && s2.sel.adLive === true && s2.sel.selTrue === 1 &&
      s2.errBanner === false && pageErrors.length === 0,
    's1=' + JSON.stringify(s1) + ' s2=' + JSON.stringify(s2) + ' errors=' + JSON.stringify(pageErrors));
  await page.close();
});

/* =====================================================================
 * C. 壳 A 在途; 退出/重进 -> 壳 B 加载 P; 先 release A (abort 伪装 no-op,
 *    A 的响应真实 resolve) -> A 不得窃取 B 的节点 / 删除 B 的在途或意图;
 *    B 恰安装一次.
 * ===================================================================== */
await runScenario('C cross-shell: release A (abort-mocked) leaves B pending/DOM untouched; B installs exactly once', async () => {
  const { page, pageErrors } = await fresh('dir', { nohost: true, patchAbort: true });
  const holdSet = new Set(['/adir']);
  const held = new Map();
  await holdTreePath(page, holdSet, held);

  // 进入壳 A (根不 held), 点击 adir -> 请求 A held + 续接方 A
  await page.click('.cb-mode-toggle');
  await waitFor(() => page.evaluate("!!document.querySelector('ul#cb-tree-root > li.cb-tree-node')"), 'A-root-attached');
  await sleep(300);
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await waitHeld(page, held, '/adir', 1, 'A-held');
  await sleep(200);
  // 退出 (无 pane -> 不重载; abort 伪装), 重进 -> 壳 B
  await page.click('.cb-mode-toggle');
  await waitFor(() => page.evaluate("!document.body.classList.contains('cb-tree-mode')"), 'exited-A');
  await page.click('.cb-mode-toggle');
  await waitFor(() => page.evaluate("!!document.querySelector('ul#cb-tree-root > li.cb-tree-node')"), 'B-root-attached');
  await sleep(300);
  // 壳 B: 点击 adir -> 请求 B held
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await waitHeld(page, held, '/adir', 2, 'B-held');
  await sleep(300);
  const midB = { held: (held.get('/adir') || []).length, paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  // 先 release A: A 的 resolve 响应不得在壳 B 安装/删除
  await releaseOne(page, held, '/adir');
  await sleep(600);
  const afterA = {
    held: (held.get('/adir') || []).length,
    paused: pausedCounts['/adir'] || 0,
    net: netCounts['/adir'] || 0,
    state: await dirState(page, 'adir'),
    abortMock: await page.evaluate('window.__abortMock'),
  };
  // release B: B 自己的链按 B 的意图安装 (首击 -> 展开)
  await releaseOne(page, held, '/adir');
  await waitFor(() => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"adir\"] ul.cb-tree-children li')"), 'B-landed');
  await sleep(400);
  const finalB = await dirState(page, 'adir');
  const finalCounts = { paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  rec('C cross-shell: release A (abort-mocked) leaves B pending/DOM untouched; B installs exactly once',
    midB.held === 2 && midB.paused === 2 && midB.net === 2 &&
      afterA.held === 1 && afterA.paused === 2 && afterA.net === 2 &&
      afterA.state.expandable === '1' && afterA.state.expanded === 'false' && afterA.state.hasUl === false && afterA.state.twist === '▸' &&
      afterA.abortMock >= 1 &&
      finalB.children === 2 && finalB.hidden === false && finalB.expanded === 'true' && finalB.twist === '▾' &&
      finalCounts.paused === 2 && finalCounts.net === 2 && pageErrors.length === 0,
    'midB=' + JSON.stringify(midB) + ' afterA=' + JSON.stringify(afterA) + ' finalB=' + JSON.stringify(finalB) +
      ' finalCounts=' + JSON.stringify(finalCounts) + ' errors=' + JSON.stringify(pageErrors));
  await page.close();
});

/* =====================================================================
 * D. 已缓存折叠祖先 -> 面板下钻子项: 祖先缓存命中展开 (零新请求), 子项
 *    current + 选中.
 * ===================================================================== */
await runScenario('D cached collapsed ancestor -> pane navigate child: ancestor expands, child current/selected', async () => {
  const { page, pageErrors } = await fresh('tree');
  const holdSet = new Set([]);
  const held = new Map();
  await holdTreePath(page, holdSet, held);
  // 展开 adir (加载落地), 再折叠 (无在途 -> DOM 即权威, 缓存仍在)
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await waitFor(() => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"adir/child.md\"]')"), 'adir-children');
  await sleep(300);
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await waitFor(() => page.evaluate("document.querySelector('[data-native-path-encoded=\"adir\"] > ul.cb-tree-children')?.hidden === true"), 'adir-collapsed');
  const collapsed = await dirState(page, 'adir');
  // 面板迷你列表: /adir 再 /adir/child.md (祖先 reveal 走缓存命中)
  await waitFor(() => page.evaluate("!!document.querySelector('.cb-pane .cb-dir-mini a[href=\"/adir\"]')"), 'pane-mini-ready');
  await page.click('.cb-pane .cb-dir-mini a[href="/adir"]');
  await waitFor(() => page.evaluate("location.pathname === '/adir'"), 'nav-adir');
  const afterDirNav = await dirState(page, 'adir');
  await waitFor(() => page.evaluate("!!document.querySelector('.cb-pane .cb-dir-mini a[href=\"/adir/child.md\"]')"), 'pane-child-mini-ready');
  await page.click('.cb-pane .cb-dir-mini a[href="/adir/child.md"]');
  await waitFor(() => page.evaluate("location.pathname === '/adir/child.md'"), 'nav-child');
  await waitFor(() => page.evaluate("(() => { const li = document.querySelector('[data-native-path-encoded=\"adir\"]'); return li && li.getAttribute('aria-expanded') === 'true' && li.querySelector(':scope > ul.cb-tree-children')?.hidden === false; })()"), 'adir-reveal-expanded');
  await sleep(300);
  const s = {
    adir: await dirState(page, 'adir'),
    current: await currentPaths(page),
    sel: await selState(page),
  };
  rec('D cached collapsed ancestor -> pane navigate child: ancestor expands, child current/selected',
    collapsed.expanded === 'false' && collapsed.hidden === true &&
      afterDirNav.expanded === 'true' && afterDirNav.hidden === false &&
      s.adir.expanded === 'true' && s.adir.hidden === false && s.adir.children === 2 &&
      JSON.stringify(s.current) === JSON.stringify(['adir/child.md']) &&
      s.sel.selected === 'adir/child.md' && s.sel.adLive === true && s.sel.selTrue === 1 &&
      pageErrors.length === 0,
    'collapsed=' + JSON.stringify(collapsed) + ' afterDirNav=' + JSON.stringify(afterDirNav) +
      ' s=' + JSON.stringify(s) + ' errors=' + JSON.stringify(pageErrors));
  await page.close();
});

/* =====================================================================
 * E. 空目录持久化展开含重复条目: 叶子化移除 ALL 出现; 无 disclosure;
 *    转换后点击/ArrowRight 零额外请求 (恰一次请求).
 * ===================================================================== */
await runScenario('E duplicated persisted empty dir: ALL occurrences removed, no disclosure, one request', async () => {
  const { page, pageErrors } = await fresh('dir');
  await page.evaluate("localStorage.setItem('cb-tree-expanded', JSON.stringify(['emptydir', 'emptydir']))");
  await page.goto(BASE + '/'); // 重载: 进入链读取种子展开
  const holdSet = new Set(['/']);
  const held = new Map();
  await holdTreePath(page, holdSet, held, null);
  // 根 JSON held 时进入树模式
  await page.click('.cb-mode-toggle');
  await waitHeld(page, held, '/', 1, 'E-root-held');
  await sleep(200);
  // 面板初始 '/' fragment 独立落地; 先面板导航 -> /adir 使
  // paneCommittedGen != paneInitialGen -> 进入链跳过持久化 walk
  await waitFor(() => page.evaluate("!!document.querySelector('.cb-pane .cb-dir-mini a[href=\"/adir\"]')"), 'pane-mini-ready');
  await page.click('.cb-pane .cb-dir-mini a[href="/adir"]');
  await waitFor(() => page.evaluate("location.pathname === '/adir'"), 'E-nav-adir');
  await sleep(200);
  // release 根: 链重同步到 /adir (不做 emptydir 的持久化 walk)
  await releaseOne(page, held, '/');
  await waitFor(() => page.evaluate("!!document.querySelector('ul#cb-tree-root > li.cb-tree-node')"), 'root-attached');
  await waitFor(() => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"adir\"] ul.cb-tree-children li')"), 'E-adir-loaded');
  await sleep(300);
  // 单击驱动叶子化: 内存列表仍持两个重复条目
  await page.click('[data-native-path-encoded="emptydir"] .cb-tree-row');
  await waitFor(() => page.evaluate("document.querySelector('[data-native-path-encoded=\"emptydir\"]')?.dataset.treeExpandable === '0'"), 'emptydir-single-leaf');
  await sleep(250);
  const leaf = await page.evaluate(
    '(() => { const li = document.querySelector("[data-native-path-encoded=\\"emptydir\\"]"); return { expandable: li.dataset.treeExpandable, hasAria: li.hasAttribute("aria-expanded"), twist: li.querySelector(".cb-tree-twist").textContent, groupUls: li.querySelectorAll(":scope > ul[role=\\"group\\"]").length }; })()'
  );
  const list = await expandedList(page);
  const counts = { paused: pausedCounts['/emptydir'] || 0, net: netCounts['/emptydir'] || 0 };
  // 转换后: 再点击 + ArrowRight (根聚焦) -> 零额外请求
  await page.click('[data-native-path-encoded="emptydir"] .cb-tree-row');
  await page.evaluate("document.getElementById('cb-tree-root').focus()");
  await page.press('ArrowRight', 'ArrowRight', 39);
  await sleep(300);
  const counts2 = { paused: pausedCounts['/emptydir'] || 0, net: netCounts['/emptydir'] || 0 };
  rec('E duplicated persisted empty dir: ALL occurrences removed, no disclosure, one request',
    leaf.expandable === '0' && leaf.hasAria === false && leaf.twist === '' && leaf.groupUls === 0 &&
      Array.isArray(list) && !list.includes('emptydir') &&
      counts.paused === 1 && counts.net === 1 && counts2.paused === 1 && counts2.net === 1 &&
      pageErrors.length === 0,
    'leaf=' + JSON.stringify(leaf) + ' list=' + JSON.stringify(list) + ' counts=' + JSON.stringify(counts) +
      ' counts2=' + JSON.stringify(counts2) + ' errors=' + JSON.stringify(pageErrors));
  await page.close();
});

/* =====================================================================
 * F. 根端点失败: 可见纯文本 role=alert 错误 (nav 内, 树 ul 兄弟), 根清空,
 *    零未处理拒绝; 重进新壳成功恢复, 无错误残留.
 * ===================================================================== */
await runScenario('F root endpoint fail: visible role=alert error, zero unhandledrejection; retry/new shell works', async () => {
  const { page, pageErrors } = await fresh('dir');
  await page.evaluate(
    "(() => { document.querySelector('main.cb-main')?.remove(); document.querySelector('.cb-doc')?.remove(); window.__uhr = []; window.addEventListener('unhandledrejection', (e) => { window.__uhr.push(String(e.reason && e.reason.message ? e.reason.message : e.reason)); }); })()"
  );
  const holdSet = new Set([]);
  const held = new Map();
  const failOnce = { done: false };
  await holdTreePath(page, holdSet, held, failOnce);
  await page.click('.cb-mode-toggle');
  await waitFor(() => page.evaluate("!!document.querySelector('#cb-tree-nav .cb-tree-error')"), 'root-error-visible');
  await sleep(300);
  const errState = await page.evaluate(
    '(() => { const nav = document.getElementById("cb-tree-nav"); const root = document.getElementById("cb-tree-root"); const err = nav ? nav.querySelector(".cb-tree-error") : null; return { exists: !!err, role: err ? err.getAttribute("role") : null, text: err ? err.textContent : null, parentIsNav: !!err && err.parentElement === nav, siblingOfRoot: !!err && err.parentElement === root.parentElement, insideRootUl: !!err && root.contains(err), rootChildren: root ? root.querySelectorAll(":scope > li").length : -1, uhr: window.__uhr, mode: document.body.classList.contains("cb-tree-mode") }; })()'
  );
  // 重试: 退出 + 重进 -> 新壳根请求成功
  await page.click('.cb-mode-toggle');
  await waitFor(() => page.evaluate("!document.body.classList.contains('cb-tree-mode')"), 'exited-F');
  await page.click('.cb-mode-toggle');
  await waitFor(() => page.evaluate("document.querySelectorAll('#cb-tree-root > li').length > 0"), 'retry-root-landed');
  await sleep(400);
  const retryState = await page.evaluate(
    '(() => { const root = document.getElementById("cb-tree-root"); return { children: root ? root.querySelectorAll(":scope > li").length : -1, hasError: root ? !!root.querySelector(".cb-tree-error") : null, uhr: window.__uhr }; })()'
  );
  rec('F root endpoint fail: visible role=alert error, zero unhandledrejection; retry/new shell works',
    errState.exists && errState.role === 'alert' && errState.text === '无法加载目录树' &&
      errState.parentIsNav && errState.siblingOfRoot && errState.insideRootUl === false &&
      errState.rootChildren === 0 && errState.uhr.length === 0 && errState.mode === true &&
      retryState.children === 4 && retryState.hasError === false && retryState.uhr.length === 0 &&
      pageErrors.length === 0,
    'err=' + JSON.stringify(errState) + ' retry=' + JSON.stringify(retryState) + ' errors=' + JSON.stringify(pageErrors));
  await page.close();
});

/* =====================================================================
 * G. 行点击: 指针焦点落树根 (preventScroll, 单 tab 停靠点) + 键盘立即可用
 *    (ArrowUp 移动选中); ARIA: 字形 (twist/icon/badge) aria-hidden, bdi 保留,
 *    可访问名不含字形.
 * ===================================================================== */
await runScenario('G row click focuses tree root (preventScroll) + keyboard works; accessible name excludes glyphs', async () => {
  const { page, pageErrors } = await fresh('tree');
  const holdSet = new Set([]);
  const held = new Map();
  await holdTreePath(page, holdSet, held);
  // 行点击导航 a.md -> 焦点落树根
  await page.click('[data-native-path-encoded="a.md"] .cb-tree-row');
  await waitFor(() => page.evaluate("location.pathname === '/a.md'"), 'click-nav-a.md');
  await sleep(250);
  const focus1 = await page.evaluate(
    '(() => ({ activeId: document.activeElement ? document.activeElement.id : null, isRoot: document.activeElement === document.getElementById("cb-tree-root") }))()'
  );
  // 键盘交接: a.md (idx 3) ArrowUp -> emptydir (idx 2)
  await page.press('ArrowUp', 'ArrowUp', 38);
  await sleep(200);
  const selAfter = await selState(page);
  // ARIA 属性契约: 字形 aria-hidden, 文件名 bdi 是唯一可访问名来源
  const attrs = await page.evaluate(
    '(() => { const li = document.querySelector("[data-native-path-encoded=\\"a.md\\"]"); const tw = li.querySelector(":scope > .cb-tree-row .cb-tree-twist"); const ic = li.querySelector(":scope > .cb-tree-row .cb-tree-icon"); const bd = li.querySelector(":scope > .cb-tree-row .cb-tree-badge"); const bdi = li.querySelector(":scope > .cb-tree-row bdi"); const dir = document.querySelector("[data-native-path-encoded=\\"adir\\"]"); const dtw = dir.querySelector(":scope > .cb-tree-row .cb-tree-twist"); return { twistHidden: tw.getAttribute("aria-hidden"), iconHidden: ic.getAttribute("aria-hidden"), badgeHidden: bd ? bd.getAttribute("aria-hidden") : null, bdiHidden: bdi.getAttribute("aria-hidden"), dirTwistHidden: dtw.getAttribute("aria-hidden"), dirTwistText: dtw.textContent }; })()'
  );
  // 可访问名契约 (CDP Accessibility 树)
  const doc = await page.send('DOM.getDocument');
  const fileNode = await page.send('DOM.querySelector', { nodeId: doc.root.nodeId, selector: '[data-native-path-encoded="a.md"]' });
  const fileAx = await page.send('Accessibility.getPartialAXTree', { nodeId: fileNode.nodeId, fetchRelatives: false });
  const dirNode = await page.send('DOM.querySelector', { nodeId: doc.root.nodeId, selector: '[data-native-path-encoded="adir"]' });
  const dirAx = await page.send('Accessibility.getPartialAXTree', { nodeId: dirNode.nodeId, fetchRelatives: false });
  const nameOf = (ax) => (ax.nodes && ax.nodes[0] && ax.nodes[0].name && ax.nodes[0].name.value) || '';
  const fileName = nameOf(fileAx);
  const dirName = nameOf(dirAx);
  rec('G row click focuses tree root (preventScroll) + keyboard works; accessible name excludes glyphs',
    focus1.isRoot === true && focus1.activeId === 'cb-tree-root' &&
      selAfter.selected === 'emptydir' && selAfter.adLive === true &&
      attrs.twistHidden === 'true' && attrs.iconHidden === 'true' && attrs.badgeHidden === 'true' &&
      attrs.bdiHidden === null && attrs.dirTwistHidden === 'true' && attrs.dirTwistText === '▸' &&
      fileName === 'a.md' && dirName === 'adir' &&
      pageErrors.length === 0,
    'focus=' + JSON.stringify(focus1) + ' selAfter=' + JSON.stringify(selAfter) + ' attrs=' + JSON.stringify(attrs) +
      ' fileAXName=' + JSON.stringify(fileName) + ' dirAXName=' + JSON.stringify(dirName) + ' errors=' + JSON.stringify(pageErrors));
  await page.close();
});

/* =====================================================================
 * N. 陈旧 nav-only 成功落地 (无续接方): 响应只写缓存未安装 — 缓存存在、
 *    在途清除、默认 desired/显式标记不残留; 后续当前导航缓存命中展开,
 *    零额外请求 (默认 nav/reveal desired 不留壳生命周期).
 * ===================================================================== */
await runScenario('N stale nav-only settlement: cache exists/pending gone; later cache-hit reveal expands with zero extra requests', async () => {
  const { page, pageErrors } = await fresh('tree');
  const holdSet = new Set(['/adir']);
  const held = new Map();
  await holdTreePath(page, holdSet, held);

  // nav A: 面板 -> /adir (held; nav 默认 desired=true)
  await waitFor(() => page.evaluate("!!document.querySelector('.cb-pane .cb-dir-mini a[href=\"/adir\"]')"), 'pane-mini-ready');
  await page.click('.cb-pane .cb-dir-mini a[href="/adir"]');
  await waitHeld(page, held, '/adir', 1, 'N-adir-held');
  await sleep(300);
  // nav B: 树行点击 a.md -> 提交 (nav A 陈旧; 无任何用户目录点击)
  await page.click('[data-native-path-encoded="a.md"] .cb-tree-row');
  await waitFor(() => page.evaluate("location.pathname === '/a.md'"), 'N-commit-B');
  await sleep(300);
  const mid = { held: (held.get('/adir') || []).length, paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  // release A: 陈旧 nav-only 成功落地 -> 只写缓存, 零安装; finally 清在途 +
  //   默认 desired (无显式标记)
  await releaseOne(page, held, '/adir');
  await sleep(600);
  const after = {
    held: (held.get('/adir') || []).length,
    paused: pausedCounts['/adir'] || 0,
    net: netCounts['/adir'] || 0,
    state: await dirState(page, 'adir'),
  };
  // 后续当前导航: history.back() -> /adir (popstate -> paneNavigate -> reveal):
  //   缓存命中展开, 绝不重新请求 (在途已清, 缓存可共享)
  await page.evaluate('history.back()');
  await waitFor(() => page.evaluate("location.pathname === '/adir'"), 'N-back-adir');
  await waitFor(() => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"adir\"] ul.cb-tree-children li')"), 'N-adir-reveal-landed');
  await sleep(400);
  const final = {
    adir: await dirState(page, 'adir'),
    current: await currentPaths(page),
    sel: await selState(page),
    paused: pausedCounts['/adir'] || 0,
    net: netCounts['/adir'] || 0,
  };
  rec('N stale nav-only settlement: cache exists/pending gone; later cache-hit reveal expands with zero extra requests',
    mid.held === 1 && mid.paused === 1 && mid.net === 1 &&
      after.held === 0 && after.paused === 1 && after.net === 1 &&
      after.state.expanded === 'false' && after.state.hasUl === false && after.state.twist === '▸' &&
      final.adir.children === 2 && final.adir.hidden === false && final.adir.expanded === 'true' && final.adir.twist === '▾' &&
      JSON.stringify(final.current) === JSON.stringify(['adir']) &&
      final.sel.selected === 'adir' && final.sel.adLive === true &&
      final.paused === 1 && final.net === 1 && pageErrors.length === 0,
    'mid=' + JSON.stringify(mid) + ' after=' + JSON.stringify(after) + ' final=' + JSON.stringify(final) +
      ' errors=' + JSON.stringify(pageErrors));
  await page.close();
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
  cleanup();
}
