// 统一顶部图标工具区与三视图/显示规则验收 — 确定性 CDP 浏览器验收.
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
//   T1 目录列表默认: 统一顶栏 (搜索表单 + tree/list/grid 图标组 + 隐藏开关 +
//       12 项排序入口), 页面语义元数据; 旧模式按钮/工具栏行已删除
//   T2 视图切换: 浏览->树同一位置 replace (无重载); 树->网格整页补能力
//       handoff (URL/历史项不变); 工具区在树模式仍位于树之上
//   T3 v 键循环 list->grid->tree->list; 搜索框聚焦/修饰键组合时 v 不切换视图
//   T4 隐藏开关: 同历史项内更新规则/URL/偏好; 活动文档保持同一 DOM, 零导航/
//       零 fragment 重取
//   T5 排序入口: Size:Desc 同历史项内生效 (URL/history/偏好/顺序/当前项一致)
//   T5b 已打开文档 × 列表/网格 × 排序/隐藏: 列表原地更新, 活动文档不重转换
//   T6 独立阅读导航: 按保存视图回所在目录仅定位; 隐藏过滤不变; 进树可后退恢复阅读位置
//   T7 搜索页: 树视图右侧安全搜索片段; 换视图/补能力保留 q/sort/hidden 与历史
//   T8 无 JS: 隐藏/排序链接与真实表单提交跨导航保留必要字段
//
// 本脚本显式禁用 BackForwardCache (--disable-features) 以隔离确定性场景;
// 允许 BFCache 的 persisted pageshow 生命周期在 native-open 验收脚本中验证.

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
  writeFileSync(join(root, 'big.bin'), 'B'.repeat(4096));
  writeFileSync(join(root, 'leaf.md'), '# Leaf\n\nleaf page.\n');
  writeFileSync(join(root, '.hidden.txt'), 'hidden secret\n');
  mkdirSync(join(root, '.hiddendir'), { recursive: true });
  writeFileSync(join(root, '.hiddendir', 'secret.md'), '# Secret\n\nhidden dir page.\n');
  mkdirSync(join(root, 'adir', 'grand'), { recursive: true });
  writeFileSync(join(root, 'adir', 'child.md'), '# Child\n\nchild page.\n');
  writeFileSync(
    join(root, 'adir', 'reading.md'),
    '---\ntitle: An alternate document title\n---\n\n' +
      Array.from({ length: 60 }, (_, i) =>
        `## Section ${i + 1}\n\n${'Reading navigation preserves this paragraph. '.repeat(16)}\n\n`
      ).join('')
  );
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
let browserCdp = null;
let BASE = null;
let shutdownPromise = null;

// 等子进程真正退出 (收到 signal 后仍可能处于写入 profile 的关闭阶段).
function waitExit(child, timeoutMs) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return Promise.resolve(true);
  return new Promise((resolve) => {
    const onExit = () => {
      clearTimeout(timer);
      resolve(true);
    };
    const timer = setTimeout(() => {
      child.removeListener('exit', onExit);
      resolve(child.exitCode !== null || child.signalCode !== null);
    }, timeoutMs);
    child.once('exit', onExit);
  });
}

// 确定性收尾: 关 CDP -> 终止并等待子进程 -> 删 profile/fixture. 幂等;
// 收尾自身失败只告警, 不掩盖真实断言错误.
function shutdown() {
  if (!shutdownPromise) {
    shutdownPromise = doShutdown().catch((err) => {
      console.error('WARN: shutdown failed: ' + (err && err.message ? err.message : String(err)));
    });
  }
  return shutdownPromise;
}

async function doShutdown() {
  if (browserCdp) {
    try {
      browserCdp.close();
    } catch {
      /* 已关闭 */
    }
    browserCdp = null;
  }
  if (server) {
    try {
      server.kill('SIGTERM');
    } catch {
      /* 已退出 */
    }
    if (!(await waitExit(server, 2000))) {
      try {
        server.kill('SIGKILL');
      } catch {
        /* 已退出 */
      }
      await waitExit(server, 2000);
    }
    server = null;
  }
  if (chrome) {
    try {
      chrome.kill('SIGTERM');
    } catch {
      /* 已退出 */
    }
    // Chrome 关闭阶段仍会写 profile: 等它真正退出后再删, 否则残留临时目录.
    if (!(await waitExit(chrome, 4000))) {
      try {
        chrome.kill('SIGKILL');
      } catch {
        /* 已退出 */
      }
      await waitExit(chrome, 2000);
    }
    chrome = null;
  }
  if (chromeProfile) {
    let lastErr = null;
    for (let i = 0; i < 20; i++) {
      try {
        rmSync(chromeProfile, { recursive: true, force: true });
        lastErr = null;
        break;
      } catch (err) {
        lastErr = err;
        await sleep(100);
      }
    }
    if (lastErr) {
      console.error('WARN: chrome profile cleanup failed (left for OS): ' + lastErr.message);
    }
    chromeProfile = null;
  }
  if (fixture) {
    try {
      rmSync(fixture, { recursive: true, force: true });
    } catch (err) {
      console.error('WARN: fixture cleanup failed (left for OS): ' + err.message);
    }
    fixture = null;
  }
}

// 进程退出兜底 (正常/失败路径已由 shutdown 收尾): 仅同步信号 + 尽力删除.
function cleanup() {
  if (server) {
    try {
      server.kill('SIGKILL');
    } catch {
      /* 已退出 */
    }
    server = null;
  }
  if (chrome) {
    try {
      chrome.kill('SIGKILL');
    } catch {
      /* 已退出 */
    }
    chrome = null;
  }
  if (chromeProfile) {
    try {
      rmSync(chromeProfile, { recursive: true, force: true });
    } catch {
      /* 退出兜底不重试 */
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
  shutdown().finally(() => process.exit(130));
});
process.on('SIGTERM', () => {
  shutdown().finally(() => process.exit(143));
});

/* ---------- 启动: fixture + server + chrome + CDP ---------- */

// 先解析外部前提 (二进制/Chrome), 再分配 fixture/server/chrome — 初始化失败
// 不留下需要回收的临时资源 (fixture 在 makeFixture 之前无占用).
const BIN = process.env.CB_BIN || join(import.meta.dirname, '..', 'target', 'release', 'chapbook');
if (!existsSync(BIN)) {
  throw new Error('missing ' + BIN + ' — run `cargo build --release` first');
}
const chromePath = findChrome();
if (!chromePath) {
  throw new Error('no Chrome found — set CHROME_BIN or install ms-playwright chromium');
}

// 启动步骤整体可失败: 任一步失败先确定性收尾 (server/chrome/profile/fixture)
// 再上报, 不用等进程退出兜底.
async function boot() {
  fixture = makeFixture();
  const port = await pickPort();
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
  BASE = 'http://127.0.0.1:' + port;

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
  browserCdp = new Cdp('ws://127.0.0.1:' + cdpPort + wsPath);
  await browserCdp.open();
}

try {
  await boot();
} catch (err) {
  await shutdown();
  throw err;
}

/* ---------- 通用场景工具 ---------- */

let activePage = null;

async function runScenario(name, fn) {
  let failure = null;
  try {
    await fn();
  } catch (err) {
    failure = err;
  } finally {
    if (activePage) {
      await activePage.close();
      activePage = null;
    }
  }
  if (failure) {
    // 断言失败: 先确定性收尾 (server/chrome/profile/fixture) 再上报.
    await shutdown();
    throw failure;
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

/* ---------- 界面/状态查询与交互工具 ---------- */

async function waitFor(fn, label = '?', timeoutMs = 10000, step = 50) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    try {
      const v = await fn();
      if (v) return v;
    } catch {
      /* 元素/状态未就绪 */
    }
    await sleep(step);
  }
  throw new Error('waitFor timeout: ' + label);
}

// 下一次整页 load (点击/按键触发导航前注册; 未导航则超时失败).
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

// 点击视图图标进入树视图并等待根列表落地 (同一位置 replace, 不整页重载).
async function enterTreeView(page, waitMs = 400) {
  if (!(await page.evaluate("document.body.classList.contains('cb-tree-mode')"))) {
    await page.click('[data-cb-view="tree"]');
  }
  await waitFor(
    () => page.evaluate("!!document.querySelector('ul#cb-tree-root > li.cb-tree-node')"),
    'tree root attached'
  );
  await sleep(waitMs);
}

async function pressKey(page, key, code, vk, text) {
  const keyDown = {
    type: 'keyDown',
    key,
    code,
    windowsVirtualKeyCode: vk,
    nativeVirtualKeyCode: vk,
  };
  // 字符键必须带 text 才走真实输入默认行为; 视图切换自身不依赖 text.
  if (text !== undefined) {
    keyDown.text = text;
    keyDown.unmodifiedText = text;
  }
  await page.send('Input.dispatchKeyEvent', keyDown);
  await page.send('Input.dispatchKeyEvent', {
    type: 'keyUp',
    key,
    code,
    windowsVirtualKeyCode: vk,
    nativeVirtualKeyCode: vk,
  });
}

// 完整 UI/状态快照: 视图按下态、隐藏开关、排序入口、页面语义、URL/历史、偏好.
const uiState = (page) =>
  page.evaluate(
    '(() => { const views = {}; document.querySelectorAll("[data-cb-view]").forEach((el) => { views[el.getAttribute("data-cb-view")] = { pressed: el.getAttribute("aria-pressed"), disabled: el.getAttribute("aria-disabled"), label: el.getAttribute("aria-label"), tag: el.tagName }; }); const toggle = document.querySelector("[data-cb-hidden-toggle]"); const sort = document.querySelector("details.cb-sort"); const cur = sort ? sort.querySelector("[data-cb-sort][aria-current=true]") : null; return { path: location.pathname, search: location.search, len: history.length, treeMode: document.body.classList.contains("cb-tree-mode"), views, hidden: toggle ? { tag: toggle.tagName, pressed: toggle.getAttribute("aria-pressed"), disabled: toggle.getAttribute("aria-disabled"), href: toggle.getAttribute("href") } : null, sortDisabled: sort ? sort.getAttribute("aria-disabled") : null, sortCount: sort ? sort.querySelectorAll("[data-cb-sort]").length : -1, sortCurrent: cur ? cur.getAttribute("data-cb-sort") : null, surface: document.body.dataset.cbSurface, sort: document.body.dataset.cbSort, hiddenRule: document.body.dataset.cbHidden, query: document.body.dataset.cbQuery === undefined ? null : document.body.dataset.cbQuery, marker: window.__docMarker === undefined ? null : window.__docMarker, hiddenPref: localStorage.getItem("cb-hidden"), sortPref: localStorage.getItem("cb-sort"), viewPref: localStorage.getItem("cb-browse-view"), legacyMode: localStorage.getItem("cb-mode"), legacyView: localStorage.getItem("cb-view") }; })()'
  );

// 行条目顺序 (目录表) — data-native-path-encoded 序列.
const rowOrder = (page) =>
  page.evaluate(
    '[...document.querySelectorAll("tr.cb-row[data-browser-entry]")].map((el) => el.getAttribute("data-native-path-encoded"))'
  );

/* ---------- 无 JS 工具 (脚本执行禁用后仅 DOM/Input/Page 域可用) ---------- */

async function noJsHtml(page) {
  await page.send('DOM.enable');
  const { root } = await page.send('DOM.getDocument', { depth: 1 });
  const { outerHTML } = await page.send('DOM.getOuterHTML', { nodeId: root.nodeId });
  return outerHTML;
}

async function noJsBox(page, selector) {
  const { root } = await page.send('DOM.getDocument', { depth: 1 });
  const { nodeId } = await page.send('DOM.querySelector', { nodeId: root.nodeId, selector });
  if (!nodeId) throw new Error('noJs: no element for ' + selector);
  const { model } = await page.send('DOM.getBoxModel', { nodeId });
  const c = model.content;
  return {
    nodeId,
    x: (c[0] + c[2] + c[4] + c[6]) / 4,
    y: (c[1] + c[3] + c[5] + c[7]) / 4,
  };
}

async function noJsClick(page, selector) {
  const { x, y } = await noJsBox(page, selector);
  await page.send('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', clickCount: 1 });
  await page.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', clickCount: 1 });
}

async function noJsUrl(page) {
  const { currentIndex, entries } = await page.send('Page.getNavigationHistory');
  return entries[currentIndex].url;
}

async function noJsGoto(page, url) {
  const loaded = waitForLoad(page);
  await page.send('Page.navigate', { url });
  await loaded;
  await sleep(200);
}

async function noJsWaitLoadAfter(page, action) {
  const loaded = waitForLoad(page);
  await action();
  await loaded;
  await sleep(200);
}

/* =====================================================================
 * T1 冷启动目录: 默认视图/显示规则有效; 无旧偏好时不写入任何持久偏好.
 * ===================================================================== */
await runScenario('T1 cold directory: defaults apply without storing display preferences', async () => {
  const { page, pageErrors } = await fresh();
  const s = await uiState(page);
  const ok =
    s.surface === 'directory' &&
    s.sort === 'Name:Asc' &&
    s.hiddenRule === '0' &&
    s.views.list.pressed === 'true' &&
    s.views.tree.pressed === 'false' &&
    s.views.grid.pressed === 'false' &&
    s.views.tree.disabled !== 'true' &&
    s.views.grid.disabled !== 'true' &&
    s.hidden &&
    s.hidden.pressed === 'false' &&
    typeof s.hidden.href === 'string' &&
    s.hidden.href.includes('hidden=1') &&
    s.sortCount === 12 &&
    s.sortCurrent === 'Name:Asc' &&
    s.viewPref === null &&
    s.sortPref === null &&
    s.hiddenPref === null &&
    pageErrors.length === 0;
  rec(
    'T1 cold directory: defaults apply without storing display preferences',
    ok,
    JSON.stringify({ s, errors: pageErrors })
  );
});

/* =====================================================================
 * T2 视图切换: 浏览->树是同一位置 replace (无重载); 树->网格是整页补能力
 *    handoff (URL/历史项不变, 真实重载); 工具区在树模式仍可见于树/网格之上;
 *    list<->grid 同一位置 replace.
 * ===================================================================== */
await runScenario('T2 view switch: browse->tree replace without reload; tree->grid full-page handoff keeps URL/history; toolbar above tree shell', async () => {
  const { page, pageErrors } = await fresh();
  await page.goto(BASE + '/?sort=Name:Asc&hidden=0');
  const before = await uiState(page);
  await page.evaluate("window.__docMarker = 'T2'");
  await enterTreeView(page);
  const tree = await uiState(page);
  const layout = await page.evaluate(
    '(() => { const bar = document.querySelector("header.cb-topbar"); const nav = document.getElementById("cb-tree-nav"); const b = bar.getBoundingClientRect(); const n = nav.getBoundingClientRect(); return { barVisible: getComputedStyle(bar).display !== "none", barBottom: Math.round(b.bottom), navTop: Math.round(n.top) }; })()'
  );
  // 树 -> 网格: 整页 handoff (replace 当前历史项 + reload, 不新增历史)
  const loaded = waitForLoad(page);
  await page.click('[data-cb-view="grid"]');
  await loaded;
  await sleep(300);
  const grid = await uiState(page);
  const gridSections = await page.evaluate(
    '(() => { const g = document.getElementById("cb-view-grid"); const l = document.getElementById("cb-view-list"); const t = document.getElementById("cb-tree-nav"); return { grid: g ? !g.hidden : false, list: l ? l.hidden : null, tree: !!t }; })()'
  );
  // 网格 -> 列表: 同一位置 replace (无 reload, 标记保留)
  await page.evaluate("window.__docMarker = 'T2b'");
  await page.click('[data-cb-view="list"]');
  await sleep(300);
  const list = await uiState(page);
  const ok =
    tree.treeMode &&
    tree.views.tree.pressed === 'true' &&
    tree.views.list.pressed === 'false' &&
    tree.marker === 'T2' &&
    tree.search === before.search &&
    layout.barVisible &&
    layout.barBottom <= layout.navTop &&
    grid.marker === null &&
    grid.treeMode === false &&
    grid.views.grid.pressed === 'true' &&
    grid.path === '/' &&
    grid.search === before.search &&
    grid.len === tree.len &&
    gridSections.grid &&
    gridSections.list === true &&
    gridSections.tree === false &&
    list.marker === 'T2b' &&
    list.treeMode === false &&
    list.views.list.pressed === 'true' &&
    list.search === before.search &&
    list.len === grid.len &&
    pageErrors.length === 0;
  rec(
    'T2 view switch: browse->tree replace without reload; tree->grid full-page handoff keeps URL/history; toolbar above tree shell',
    ok,
    JSON.stringify({ before, tree, layout, grid, gridSections, list, errors: pageErrors })
  );
});

/* =====================================================================
 * T3 v 键循环 list->grid->tree->list (树->列表整页 handoff); 搜索框聚焦或
 *    修饰键组合时 v 原样输入/不切换视图.
 * ===================================================================== */
await runScenario('T3 v cycles list->grid->tree->list; editable focus keeps literal v', async () => {
  const { page, pageErrors } = await fresh();
  await page.goto(BASE + '/?sort=Name:Asc&hidden=0');
  await page.evaluate("window.__docMarker = 'T3'");
  await pressKey(page, 'v', 'KeyV', 86);
  await sleep(250);
  const grid = await uiState(page);
  await pressKey(page, 'v', 'KeyV', 86);
  await waitFor(() => page.evaluate("document.body.classList.contains('cb-tree-mode')"), 'tree via v');
  await sleep(250);
  const tree = await uiState(page);
  const loaded = waitForLoad(page);
  await pressKey(page, 'v', 'KeyV', 86);
  await loaded;
  await sleep(300);
  const list = await uiState(page);
  // 整页 handoff 后先等 JS 初始化完成 (hidden 开关 href 被规范化成根相对
  // 路径是初始化已执行的标志), 否则按键守卫可能尚未安装而空过.
  await waitFor(
    () =>
      page.evaluate(
        "(() => { const t = document.querySelector('[data-cb-hidden-toggle]'); return !!t && (t.getAttribute('href') || '').startsWith('/'); })()"
      ),
    'list page app ready'
  );
  // 搜索框聚焦: 真实按键 v 原样输入, 不切换视图 (可编辑目标守卫).
  await page.evaluate("document.querySelector('#cb-search-q').focus()");
  await pressKey(page, 'v', 'KeyV', 86, 'v');
  await sleep(200);
  const editable = await page.evaluate(
    '({ value: document.querySelector("#cb-search-q").value, active: document.activeElement ? (document.activeElement.id || document.activeElement.tagName) : null, treeMode: document.body.classList.contains("cb-tree-mode"), grid: document.getElementById("cb-view-grid") ? !document.getElementById("cb-view-grid").hidden : null })'
  );
  // 修饰键组合 (Ctrl+v) 不触发视图切换 (非可编辑焦点下验证).
  await page.evaluate("document.activeElement.blur(); document.body.focus();");
  await page.send('Input.dispatchKeyEvent', {
    type: 'keyDown',
    key: 'v',
    code: 'KeyV',
    windowsVirtualKeyCode: 86,
    nativeVirtualKeyCode: 86,
    modifiers: 2,
  });
  await page.send('Input.dispatchKeyEvent', {
    type: 'keyUp',
    key: 'v',
    code: 'KeyV',
    windowsVirtualKeyCode: 86,
    nativeVirtualKeyCode: 86,
    modifiers: 2,
  });
  await sleep(200);
  const modifier = await uiState(page);
  const ok =
    grid.views.grid.pressed === 'true' &&
    grid.marker === 'T3' &&
    grid.treeMode === false &&
    tree.treeMode &&
    tree.views.tree.pressed === 'true' &&
    tree.marker === 'T3' &&
    list.marker === null &&
    list.treeMode === false &&
    list.views.list.pressed === 'true' &&
    editable.value === 'v' &&
    editable.active === 'cb-search-q' &&
    editable.treeMode === false &&
    editable.grid === false &&
    modifier.treeMode === false &&
    modifier.views.list.pressed === 'true' &&
    modifier.views.grid.pressed === 'false' &&
    pageErrors.length === 0;
  rec(
    'T3 v cycles list->grid->tree->list; editable focus keeps literal v',
    ok,
    JSON.stringify({ grid, tree, list, editable, modifier, errors: pageErrors })
  );
});

/* =====================================================================
 * T4 隐藏开关: 同历史项内更新规则/URL/偏好 (fetch 后选择性安装, 不整页导航);
 *    已打开的文件预览保持同一活动文档 (不重转换, 不重新 fetch fragment).
 * ===================================================================== */
await runScenario('T4 hidden toggle: same-entry rules/history/pref update, live opened document preserved without navigation or fragment refetch', async () => {
  const { page, pageErrors } = await fresh();
  const before = await uiState(page);
  const requests = [];
  page.on('Network.requestWillBeSent', (params, sid) => {
    if (sid !== page.sessionId) return;
    requests.push({ url: params.request.url, type: params.type });
  });
  await page.send('Network.enable');

  // 显示隐藏项: 点文件行 (标记活动文档) 后隐藏再显示, 文档必须保持同一 DOM.
  await page.click('[data-cb-hidden-toggle]');
  await waitFor(
    () => page.evaluate("!!document.querySelector('tr.cb-row[data-native-path-encoded=\".hidden.txt\"]')"),
    'hidden row shown'
  );
  await sleep(200);
  const shown = await uiState(page);
  const shownUrls = requests.map((r) => r.url);
  await page.click('tr.cb-row[data-native-path-encoded=".hidden.txt"] [data-cb-action="preview"]');
  await waitFor(
    () =>
      page.evaluate(
        "(() => { const c = document.getElementById('cb-preview-content'); return !!c && c.textContent.includes('hidden secret'); })()"
      ),
    'hidden preview open'
  );
  const marked = await page.evaluate(
    "(() => { const doc = document.querySelector('#cb-preview-content .cb-doc'); if (!doc) return false; doc.setAttribute('data-test-live-doc', 'T4'); window.__liveDoc = doc; return true; })()"
  );
  const fragFetches = () => requests.filter((r) => r.url.includes('/.hidden.txt?') && r.url.includes('fragment=1')).length;
  const docNavigations = () => requests.filter((r) => r.type === 'Document').length;
  const fragBefore = fragFetches();
  const navigationsBefore = docNavigations();

  await page.click('[data-cb-hidden-toggle]');
  await waitFor(
    () => page.evaluate("!document.querySelector('tr.cb-row[data-native-path-encoded=\".hidden.txt\"]')"),
    'hidden row gone'
  );
  await sleep(250);
  const hiddenAgain = await uiState(page);
  const live = await page.evaluate(
    "(() => { const doc = document.querySelector('#cb-preview-content .cb-doc'); return { sameNode: !!doc && doc === window.__liveDoc, marker: doc ? doc.getAttribute('data-test-live-doc') : null, text: doc ? doc.textContent.includes('hidden secret') : false, hidden: (document.getElementById('cb-preview') || {}).hidden }; })()"
  );
  const ok =
    marked &&
    before.len === shown.len &&
    shown.len === hiddenAgain.len &&
    shown.hidden.pressed === 'true' &&
    shown.search.includes('hidden=1') &&
    shown.hiddenPref === '1' &&
    hiddenAgain.hidden.pressed === 'false' &&
    !hiddenAgain.search.includes('hidden=1') &&
    hiddenAgain.hiddenPref === '0' &&
    live.sameNode &&
    live.marker === 'T4' &&
    live.text === true &&
    live.hidden === false &&
    fragFetches() === fragBefore &&
    docNavigations() === navigationsBefore &&
    pageErrors.length === 0;
  rec(
    'T4 hidden toggle: same-entry rules/history/pref update, live opened document preserved without navigation or fragment refetch',
    ok,
    JSON.stringify({ before, shown, hiddenAgain, live, fragBefore, fragAfter: fragFetches(), navigationsBefore, navigationsAfter: docNavigations(), shownUrls, errors: pageErrors })
  );
});

/* =====================================================================
 * T5 排序入口: 12 项 + 当前项 aria-current; Size:Desc 同历史项内生效
 *    (URL/history/偏好/条目顺序/当前项一致).
 * ===================================================================== */
await runScenario('T5 keyboard sort: Size:Desc updates URL/history/pref/order in the same entry', async () => {
  const { page, pageErrors } = await fresh();
  await page.goto(BASE + '/?sort=Name:Asc&hidden=0');
  const before = await uiState(page);
  await page.click('#cb-search-q');
  for (let i = 0; i < 5; i++) await pressKey(page, 'Tab', 'Tab', 9);
  await waitFor(() => page.evaluate("document.activeElement.matches('summary[data-cb-sort-summary]')"), 'sort summary keyboard focus');
  await pressKey(page, 'Enter', 'Enter', 13, '\r');
  await waitFor(() => page.evaluate("document.querySelector('details.cb-sort').open"), 'sort menu keyboard open');
  for (let i = 0; i < 6; i++) await pressKey(page, 'Tab', 'Tab', 9);
  await waitFor(() => page.evaluate("document.activeElement.dataset.cbSort === 'Size:Desc'"), 'sort direction keyboard focus');
  await pressKey(page, 'Enter', 'Enter', 13, '\r');
  await waitFor(() => page.evaluate("location.search.includes('sort=Size')"), 'sort applied to URL');
  await sleep(250);
  const after = await uiState(page);
  const order = await rowOrder(page);
  const big = order.indexOf('big.bin');
  const md = order.indexOf('a.md');
  const ok =
    before.sortCount === 12 &&
    before.sortCurrent === 'Name:Asc' &&
    before.len === after.len &&
    after.sortCurrent === 'Size:Desc' &&
    after.search.includes('sort=Size') &&
    after.search.includes('hidden=0') &&
    after.sortPref === 'Size:Desc' &&
    big !== -1 &&
    md !== -1 &&
    big < md &&
    pageErrors.length === 0;
  rec(
    'T5 keyboard sort: Size:Desc updates URL/history/pref/order in the same entry',
    ok,
    JSON.stringify({ before, after, order, errors: pageErrors })
  );
});

/* =====================================================================
 * T5b 已打开文档 × 列表/网格 × 排序/隐藏: 同历史项内列表内容更新, 活动文档
 *     保持同一 DOM/内容/identity, 文档 fragment 请求数不增加, 零整页导航.
 * ===================================================================== */
await runScenario('T5b opened document: sort and hidden changes update listing in place for list and grid without refetching the live document', async () => {
  const { page, pageErrors } = await fresh();
  await page.goto(BASE + '/?sort=Name:Asc&hidden=0');
  const requests = [];
  page.on('Network.requestWillBeSent', (params, sid) => {
    if (sid !== page.sessionId) return;
    requests.push({ url: params.request.url, type: params.type });
  });
  await page.send('Network.enable');
  const docFragFetches = () =>
    requests.filter((r) => r.url.includes('/a.md?') && r.url.includes('fragment=1')).length;
  const docNavigations = () => requests.filter((r) => r.type === 'Document').length;
  const liveDoc = () =>
    page.evaluate(
      "(() => { const doc = document.querySelector('#cb-preview-content .cb-doc'); return { sameNode: !!doc && doc === window.__liveDoc, marker: doc ? doc.getAttribute('data-test-live-doc') : null, text: doc ? doc.textContent.includes('page a') : false, hidden: (document.getElementById('cb-preview') || {}).hidden }; })()"
    );

  // list: 打开 a.md 预览 (活动文档)
  await page.click('tr.cb-row[data-native-path-encoded="a.md"] [data-cb-action="preview"]');
  await waitFor(
    () => page.evaluate("(() => { const c = document.getElementById('cb-preview-content'); return !!c && c.textContent.includes('page a'); })()"),
    'list preview open'
  );
  const marked = await page.evaluate(
    "(() => { const doc = document.querySelector('#cb-preview-content .cb-doc'); if (!doc) return false; doc.setAttribute('data-test-live-doc', 'T5b'); window.__liveDoc = doc; return true; })()"
  );
  const listFragBefore = docFragFetches();
  const listNavBefore = docNavigations();
  await page.evaluate("window.__docMarker = 'list'");
  // 排序 (list): 列表顺序更新, 文档保持 live
  await page.click('summary[data-cb-sort-summary]');
  await sleep(150);
  await page.evaluate("document.querySelector('details.cb-sort').open = true");
  await page.click('[data-cb-sort="Size:Desc"]');
  await waitFor(
    () => page.evaluate("(() => { const rows = [...document.querySelectorAll('tr.cb-row[data-browser-entry]')].map((el) => el.getAttribute('data-native-path-encoded')); const b = rows.indexOf('big.bin'); const m = rows.indexOf('a.md'); return b !== -1 && m !== -1 && b < m; })()"),
    'list reordered by Size:Desc'
  );
  // 隐藏 (list): 点文件出现/隐藏, 文档保持 live
  await page.click('[data-cb-hidden-toggle]');
  await waitFor(
    () => page.evaluate("!!document.querySelector('tr.cb-row[data-native-path-encoded=\".hidden.txt\"]')"),
    'list hidden row shown'
  );
  const listState = await uiState(page);
  const listLive = await liveDoc();
  // 切到网格 (同位置 replace, 不重载): 文档保持 live, 网格条目按新规则排列
  await page.click('[data-cb-view="grid"]');
  await waitFor(
    () => page.evaluate("(() => { const g = document.getElementById('cb-view-grid'); return !!g && !g.hidden && !!g.querySelector('div.cb-grid-item[data-native-path-encoded=\"big.bin\"]'); })()"),
    'grid visible with reordered items'
  );
  const gridLive = await liveDoc();
  await page.evaluate("window.__docMarker = 'grid'");
  // 排序 (grid): Size:Asc 重排网格, 文档保持 live
  await page.click('summary[data-cb-sort-summary]');
  await sleep(150);
  await page.evaluate("document.querySelector('details.cb-sort').open = true");
  await page.click('[data-cb-sort="Size:Asc"]');
  await waitFor(
    () => page.evaluate("(() => { const items = [...document.querySelectorAll('#cb-view-grid div.cb-grid-item')].map((el) => el.getAttribute('data-native-path-encoded')); const b = items.indexOf('big.bin'); const m = items.indexOf('a.md'); return b !== -1 && m !== -1 && m < b; })()"),
    'grid reordered by Size:Asc'
  );
  // 隐藏 (grid): 再次隐藏, 文档保持 live
  await page.click('[data-cb-hidden-toggle]');
  await waitFor(
    () => page.evaluate("!document.querySelector('tr.cb-row[data-native-path-encoded=\".hidden.txt\"]') && !document.querySelector('#cb-view-grid div.cb-grid-item[data-native-path-encoded=\".hidden.txt\"]')"),
    'grid hidden row gone'
  );
  const gridState = await uiState(page);
  const gridLiveAfter = await liveDoc();
  const ok =
    marked &&
    listState.search.includes('sort=Size') &&
    listState.search.includes('hidden=1') &&
    listState.marker === 'list' &&
    listLive.sameNode &&
    listLive.marker === 'T5b' &&
    listLive.text === true &&
    listLive.hidden === false &&
    gridLive.sameNode &&
    gridLive.marker === 'T5b' &&
    gridLive.text === true &&
    gridLive.hidden === false &&
    gridState.sortCurrent === 'Size:Asc' &&
    gridState.search.includes('sort=Size') &&
    !gridState.search.includes('hidden=1') &&
    gridState.marker === 'grid' &&
    gridLiveAfter.sameNode &&
    gridLiveAfter.marker === 'T5b' &&
    gridLiveAfter.text === true &&
    gridLiveAfter.hidden === false &&
    docFragFetches() === listFragBefore &&
    docNavigations() === listNavBefore &&
    pageErrors.length === 0;
  rec(
    'T5b opened document: sort and hidden changes update listing in place for list and grid without refetching the live document',
    ok,
    JSON.stringify({ listState, listLive, gridState, gridLive, gridLiveAfter, listFragBefore, docFragAfter: docFragFetches(), listNavBefore, docNavAfter: docNavigations(), errors: pageErrors })
  );
});

/* =====================================================================
 * T6 阅读导航: 返回所在目录只定位, 沿用保存视图与当前规则;
 *    隐藏目标不强行显示; 阅读 -> 树新增历史并恢复原阅读位置.
 * ===================================================================== */
for (const view of ['list', 'grid', 'tree']) {
  await runScenario(`T6 reading parent: ${view} preference, locate without opening or changing preferences`, async () => {
    const { page, pageErrors } = await fresh();
    await page.evaluate(`(() => {
      localStorage.setItem('cb-browse-view', ${JSON.stringify(view)});
      localStorage.setItem('cb-sort', 'Name:Asc');
      localStorage.setItem('cb-hidden', '1');
    })()`);
    await page.goto(BASE + '/adir/reading.md?sort=Size:Desc&hidden=0');
    await waitFor(() => page.evaluate(
      "!!document.querySelector('[data-cb-reading-tree]:not([hidden])')"
    ), 'reading navigation ready');
    const reading = await page.evaluate(`(() => {
      const nav = document.querySelector('.cb-reading-nav');
      return {
        filename: nav.textContent.includes('reading.md'),
        titleInNavigation: nav.textContent.includes('An alternate document title'),
        nativeToken: !!document.querySelector('meta[name="cb-native-open-token"]')
      };
    })()`);
    const loaded = waitForLoad(page);
    await page.click('[data-cb-reading-parent]');
    await loaded;
    await waitFor(() => page.evaluate(`(() => {
      const selected = document.querySelector('[data-native-path-encoded="adir/reading.md"].cb-selected');
      return location.pathname === '/adir' && !!selected && selected.getClientRects().length > 0;
    })()`), 'file located in parent');
    const destination = await page.evaluate(`(() => {
      const preview = document.getElementById('cb-preview');
      const pane = document.querySelector('.cb-pane');
      return {
        view: history.state.cb.view,
        sort: new URL(location.href).searchParams.get('sort'),
        hidden: new URL(location.href).searchParams.get('hidden'),
        previewOpen: !!preview && !preview.hidden,
        directoryPane: !pane || !!pane.querySelector('.cb-dir-fragment'),
        opened: history.state.cb.opened,
        viewPref: localStorage.getItem('cb-browse-view'),
        sortPref: localStorage.getItem('cb-sort'),
        hiddenPref: localStorage.getItem('cb-hidden')
      };
    })()`);
    rec(`T6 reading parent: ${view} preference, locate without opening or changing preferences`,
      reading.filename && !reading.titleInNavigation && !reading.nativeToken &&
      destination.view === view && destination.sort === 'Size:Desc' && destination.hidden === '0' &&
      !destination.previewOpen && destination.directoryPane && destination.opened === null &&
      destination.viewPref === view && destination.sortPref === 'Name:Asc' && destination.hiddenPref === '1' &&
      pageErrors.length === 0,
      JSON.stringify({ reading, destination, errors: pageErrors }));
  });
}

for (const entry of [
  { path: '.hidden.txt', parent: '/', view: 'list' },
  { path: '.hiddendir/secret.md', parent: '/.hiddendir', view: 'tree' },
]) {
  await runScenario(`T6 hidden reading target: ${entry.path} does not reveal filtered entries`, async () => {
    const { page, pageErrors } = await fresh();
    await page.evaluate(`localStorage.setItem('cb-browse-view', ${JSON.stringify(entry.view)})`);
    await page.goto(BASE + '/' + entry.path + '?sort=Name:Asc&hidden=0');
    await waitFor(() => page.evaluate(
      "!!document.querySelector('[data-cb-reading-tree]:not([hidden])')"
    ), 'hidden file reading ready');
    const loaded = waitForLoad(page);
    await page.click('[data-cb-reading-parent]');
    await loaded;
    await waitFor(() => page.evaluate(
      `location.pathname === ${JSON.stringify(entry.parent)} && !!document.querySelector('[data-cb-view][aria-pressed="true"]')`
    ), 'hidden file parent ready');
    if (entry.view === 'tree') {
      await waitFor(() => page.evaluate("!!document.querySelector('.cb-pane .cb-dir-fragment')"), 'hidden parent directory pane');
    }
    const result = await page.evaluate(`(() => ({
      hidden: new URL(location.href).searchParams.get('hidden'),
      hiddenPref: localStorage.getItem('cb-hidden'),
      opened: history.state.cb.opened,
      previewOpen: !!document.querySelector('#cb-preview:not([hidden])'),
      exposed: !!document.querySelector(
        '#cb-view-list [data-native-path-encoded=".hidden.txt"], #cb-tree-root [data-native-path-encoded=".hiddendir"]'
      )
    }))()`);
    rec(`T6 hidden reading target: ${entry.path} does not reveal filtered entries`,
      result.hidden === '0' && result.hiddenPref === null && result.opened === null &&
      !result.previewOpen && !result.exposed && pageErrors.length === 0,
      JSON.stringify({ result, errors: pageErrors }));
  });
}

await runScenario('T6 reading to tree: one new history entry, back restores actual reading position, forward restores tree', async () => {
  const { page, pageErrors } = await fresh();
  await page.goto(BASE + '/adir/reading.md?sort=Name:Asc&hidden=0#section-20');
  await waitFor(() => page.evaluate(
    "!!document.querySelector('[data-cb-reading-tree]:not([hidden])')"
  ), 'long reading page ready');
  await page.waitLoad();
  // 浏览器可能在首帧调整滚动锚定；记录布局落定后的实际位置，而非指令坐标。
  const before = await page.evaluate(`new Promise((resolve) => {
    window.scrollTo({ top: 2400, behavior: 'instant' });
    requestAnimationFrame(() => requestAnimationFrame(() => {
      resolve({ length: history.length, y: scrollY, hash: location.hash });
    }));
  })`);
  await page.click('.cb-reading-desktop [data-cb-reading-tree]');
  await waitFor(() => page.evaluate(
    "document.body.classList.contains('cb-tree-mode') && !!document.querySelector('.cb-pane #section-20')"
  ), 'current document in tree');
  const tree = await page.evaluate(`(() => ({
    length: history.length,
    hash: location.hash,
    viewPref: localStorage.getItem('cb-browse-view'),
    readingNavVisible: document.querySelector('.cb-reading-nav').getClientRects().length > 0
  }))()`);
  await page.evaluate('history.back()');
  await waitFor(() => page.evaluate(`(() => {
    const nav = document.querySelector('.cb-reading-nav');
    return !document.body.classList.contains('cb-tree-mode') && !!nav && nav.getClientRects().length > 0 &&
      Math.abs(scrollY - ${before.y}) < 3;
  })()`), 'back to saved reading position');
  const back = await page.evaluate(`({
    length: history.length, y: scrollY, hash: location.hash,
    viewPref: localStorage.getItem('cb-browse-view'),
    sortPref: localStorage.getItem('cb-sort'), hiddenPref: localStorage.getItem('cb-hidden')
  })`);
  await page.evaluate('history.forward()');
  await waitFor(() => page.evaluate(
    "document.body.classList.contains('cb-tree-mode') && !!document.querySelector('.cb-pane #section-20')"
  ), 'forward to tree');
  const forward = await page.evaluate('({ length: history.length, hash: location.hash })');
  rec('T6 reading to tree: one new history entry, back restores actual reading position, forward restores tree',
    tree.length === before.length + 1 && !tree.readingNavVisible && tree.viewPref === 'tree' &&
    back.length === tree.length && back.viewPref === 'tree' && back.sortPref === null && back.hiddenPref === null &&
    forward.length === tree.length && tree.hash === before.hash && back.hash === before.hash && forward.hash === before.hash &&
    pageErrors.length === 0,
    JSON.stringify({ before, tree, back, forward, errors: pageErrors }));
});

await runScenario('T6 mobile search: slash opens its containing menu, Escape returns focus, submit preserves rules', async () => {
  const { page, pageErrors } = await fresh(390, 844);
  await page.goto(BASE + '/a.md?sort=Size:Desc&hidden=1');
  await waitFor(() => page.evaluate(
    "!!document.querySelector('[data-cb-reading-tree]:not([hidden])')"
  ), 'mobile reader ready');
  await pressKey(page, '/', 'Slash', 191, '/');
  await waitFor(() => page.evaluate(`(() => {
    const active = document.activeElement;
    return active.matches('input[type="search"]') && active.checkVisibility();
  })()`), 'visible mobile search focused');
  await pressKey(page, 'Escape', 'Escape', 27);
  await waitFor(() => page.evaluate(`(() => {
    const active = document.activeElement;
    return active.matches('.cb-reading-search > summary') &&
      !active.parentElement.open && active.checkVisibility();
  })()`), 'search focus returned to visible summary');
  await pressKey(page, '/', 'Slash', 191, '/');
  await page.send('Input.insertText', { text: 'leaf' });
  const loaded = waitForLoad(page);
  await pressKey(page, 'Enter', 'Enter', 13, '\r');
  await loaded;
  const result = await page.evaluate(`(() => {
    const url = new URL(location.href);
    return {
      path: url.pathname, q: url.searchParams.get('q'),
      sort: url.searchParams.get('sort'), hidden: url.searchParams.get('hidden'),
      leaf: !!document.querySelector('[data-browser-entry][data-native-path-encoded="leaf.md"]')
    };
  })()`);
  rec('T6 mobile search: slash opens its containing menu, Escape returns focus, submit preserves rules',
    result.path === '/__/search' && result.q === 'leaf' && result.sort === 'Size:Desc' &&
    result.hidden === '1' && result.leaf && pageErrors.length === 0,
    JSON.stringify({ result, errors: pageErrors }));
});

/* =====================================================================
 * T7 搜索页: 换视图保留 q/sort/hidden; 树视图右侧是安全搜索片段 (无 token);
 *    树->网格整页 handoff 保持 URL 与历史项数.
 * ===================================================================== */
await runScenario('T7 search page: tree pane shows safe fragment, view handoff keeps q/sort/hidden and history', async () => {
  const { page, pageErrors } = await fresh();
  await page.goto(BASE + '/__/search?q=leaf&sort=Name:Asc&hidden=0');
  const before = await uiState(page);
  const rowsBefore = await page.evaluate("document.querySelectorAll('tr.cb-row[data-browser-entry]').length");
  await page.click('tr.cb-row[data-native-path-encoded="leaf.md"] [data-cb-action="preview"]');
  await waitFor(
    () => page.evaluate("!!document.querySelector('#cb-preview:not([hidden]) [data-native-path-encoded=\"leaf.md\"]')"),
    'search document preview'
  );
  await page.evaluate("window.__docMarker = 'T7'");
  await page.click('[data-cb-view="tree"]');
  await waitFor(() => page.evaluate("!!document.querySelector('.cb-search-fragment')"), 'search fragment in pane');
  await sleep(250);
  const tree = await uiState(page);
  const frag = await page.evaluate(
    "(() => { const f = document.querySelector('.cb-search-fragment'); return { query: f.getAttribute('data-cb-query'), surface: f.getAttribute('data-cb-surface'), sort: f.getAttribute('data-cb-sort'), hidden: f.getAttribute('data-cb-hidden'), token: !!f.querySelector('meta[name=cb-native-open-token]') }; })()"
  );
  const loaded = waitForLoad(page);
  await page.click('[data-cb-view="grid"]');
  await loaded;
  await sleep(300);
  const grid = await uiState(page);
  const rowsAfter = await page.evaluate("document.querySelectorAll('tr.cb-row[data-browser-entry]').length");
  const ok =
    before.surface === 'search' &&
    before.query === 'leaf' &&
    rowsBefore > 0 &&
    tree.treeMode &&
    tree.search === before.search &&
    tree.marker === 'T7' &&
    frag.query === 'leaf' &&
    frag.surface === 'search' &&
    frag.sort === 'Name:Asc' &&
    frag.hidden === '0' &&
    !frag.token &&
    grid.marker === null &&
    grid.path === '/__/search' &&
    grid.search === before.search &&
    grid.views.grid.pressed === 'true' &&
    grid.len === tree.len &&
    rowsAfter > 0 &&
    pageErrors.length === 0;
  rec(
    'T7 search page: tree pane shows safe fragment, view handoff keeps q/sort/hidden and history',
    ok,
    JSON.stringify({ before, tree, frag, grid, rowsBefore, rowsAfter, errors: pageErrors })
  );
});

/* =====================================================================
 * T8 无 JS: 服务端字段/链接真实可用 — 隐藏开关、排序链接、搜索重搜、
 *    条目与 ../ 导航都必须携带必要字段 (实际提交/导航, 非 form 源码断言).
 * ===================================================================== */
await runScenario('T8 no-JS: hidden/sort links and real form submission preserve fields across navigation', async () => {
  const { page, pageErrors } = await fresh();
  await page.send('Emulation.setScriptExecutionDisabled', { value: true });
  await noJsGoto(page, BASE + '/?sort=Name:Asc&hidden=0');
  const html = await noJsHtml(page);
  const fieldsOk =
    html.includes('data-cb-surface="directory"') &&
    html.includes('name="sort"') &&
    html.includes('name="hidden"') &&
    html.includes('id="cb-search-q"') &&
    html.includes('data-cb-hidden-toggle') &&
    html.includes('data-cb-sort="Size:Desc"');
  // 隐藏开关: 点击 link -> 真实导航, hidden=1 且条目出现
  await noJsWaitLoadAfter(page, () => noJsClick(page, '[data-cb-hidden-toggle]'));
  const urlHidden = await noJsUrl(page);
  const htmlHidden = await noJsHtml(page);
  const hiddenNavOk = urlHidden.includes('hidden=1') && htmlHidden.includes('/.hidden.txt');
  // 排序: 展开 details (无 JS 原生 summary), 点 Size:Desc -> sort 生效且 hidden 保留
  await noJsClick(page, 'summary[data-cb-sort-summary]');
  await sleep(150);
  await noJsWaitLoadAfter(page, () => noJsClick(page, '[data-cb-sort="Size:Desc"]'));
  const urlSort = await noJsUrl(page);
  const htmlSort = await noJsHtml(page);
  const sortOk =
    urlSort.includes('sort=Size') &&
    urlSort.includes('hidden=1') &&
    htmlSort.indexOf('big.bin') !== -1 &&
    htmlSort.indexOf('big.bin') < htmlSort.indexOf('a.md');
  // 搜索: 真实字段; 重搜 (Enter 隐式提交) 保留 sort/hidden
  await noJsGoto(page, BASE + '/__/search?q=leaf&sort=Name:Asc&hidden=0');
  const searchHtml = await noJsHtml(page);
  const searchFieldsOk =
    searchHtml.includes('action="/__/search"') &&
    searchHtml.includes('name="sort"') &&
    searchHtml.includes('name="hidden"') &&
    searchHtml.includes('data-cb-query="leaf"');
  const { nodeId: qNode } = await noJsBox(page, '#cb-search-q');
  await page.send('DOM.focus', { nodeId: qNode });
  // 清空原 q 值 (无 JS 下经编辑命令全选, 再插入新查询)
  await page.send('Input.dispatchKeyEvent', {
    type: 'rawKeyDown',
    key: 'a',
    code: 'KeyA',
    windowsVirtualKeyCode: 65,
    nativeVirtualKeyCode: 65,
    modifiers: 2,
    commands: ['selectAll'],
  });
  await page.send('Input.dispatchKeyEvent', {
    type: 'keyUp',
    key: 'a',
    code: 'KeyA',
    windowsVirtualKeyCode: 65,
    nativeVirtualKeyCode: 65,
    modifiers: 2,
  });
  await page.send('Input.insertText', { text: 'a' });
  await noJsWaitLoadAfter(page, async () => {
    // 隐式提交依赖 Enter 的 text (\r) 才会走浏览器默认行为; 无 submit 按钮.
    await page.send('Input.dispatchKeyEvent', {
      type: 'keyDown',
      key: 'Enter',
      code: 'Enter',
      windowsVirtualKeyCode: 13,
      nativeVirtualKeyCode: 13,
      text: '\r',
      unmodifiedText: '\r',
    });
    await page.send('Input.dispatchKeyEvent', {
      type: 'keyUp',
      key: 'Enter',
      code: 'Enter',
      windowsVirtualKeyCode: 13,
      nativeVirtualKeyCode: 13,
    });
  });
  const urlSearch = await noJsUrl(page);
  const htmlSearch = await noJsHtml(page);
  const researchOk =
    urlSearch.includes('q=a') &&
    urlSearch.includes('sort=Name') &&
    urlSearch.includes('hidden=0') &&
    htmlSearch.includes('a.md');
  // 条目导航离开搜索: 保留 sort/hidden, 移除 q
  await noJsWaitLoadAfter(page, () => noJsClick(page, 'a[href^="/a.md?"]'));
  const urlEntry = await noJsUrl(page);
  const entryOk =
    urlEntry.includes('/a.md') && urlEntry.includes('sort=Name') && urlEntry.includes('hidden=0') && !urlEntry.includes('q=');
  // ../ 返回父目录: 保留 sort/hidden (在 /adir 上)
  await noJsGoto(page, BASE + '/adir?sort=Name:Asc&hidden=0');
  await noJsWaitLoadAfter(page, () => noJsClick(page, 'a[data-cb-parent]'));
  const urlParent = await noJsUrl(page);
  const parentOk = urlParent.includes('sort=Name') && urlParent.includes('hidden=0');
  // 搜索位置导航离开搜索: 保留 sort/hidden, 移除 q
  await noJsGoto(page, BASE + '/__/search?q=child&sort=Name:Asc&hidden=0');
  await noJsWaitLoadAfter(page, () => noJsClick(page, 'a[href^="/adir?"]'));
  const urlLocation = await noJsUrl(page);
  const locationOk = urlLocation.includes('/adir') && urlLocation.includes('sort=Name') && !urlLocation.includes('q=');
  const ok =
    fieldsOk &&
    hiddenNavOk &&
    sortOk &&
    searchFieldsOk &&
    researchOk &&
    entryOk &&
    parentOk &&
    locationOk &&
    pageErrors.length === 0;
  rec(
    'T8 no-JS: hidden/sort links and real form submission preserve fields across navigation',
    ok,
    JSON.stringify({ fieldsOk, hiddenNavOk, sortOk, searchFieldsOk, researchOk, entryOk, parentOk, locationOk, errors: pageErrors })
  );
});

/* ---------- 收尾: 确定性清理 + 汇总 ---------- */

await shutdown();
const pass = results.filter((r) => r.pass).length;
console.log('SUMMARY: ' + pass + '/' + results.length);
if (pass !== results.length) {
  results.filter((r) => !r.pass).forEach((r) => console.log('  FAILED: ' + r.name));
  process.exitCode = 1;
}
