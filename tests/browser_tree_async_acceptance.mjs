// 浏览状态契约 (版本化显示规则 + 分支状态 + 跨文档交接 + 历史/生命周期恢复)
// 确定性 CDP 浏览器验收 (A/A2/1/2/3/C/D/E/Q17T/Q17L/OH/V/H/L/R/R2/R3).
//
// 手动调用 (非 cargo test), 自包含: 仅 Node 内置模块 + 系统 Chrome 的 CDP
// WebSocket (无任何 npm 包); 自建临时 fixture (目录/文件/隐藏项/仅含隐藏孩子
// 的目录/真正空目录/两个可独立故障注入的展开分支/循环符号链接), 自行启动
// target/release/chapbook 并监听固定测试端口, finally 清理 server/chrome/
// fixture; 任一断言失败以非零退出.
//
// 运行:
//   cargo build --release && node tests/browser_tree_async_acceptance.mjs
//   CB_SCENARIO='R explicit collapse survives reload' node tests/browser_tree_async_acceptance.mjs
//
// Chrome 定位: $CHROME_BIN 优先, 其次 ms-playwright / Chrome-for-Testing 已知
// 路径 (macOS 与 Linux, 完整浏览器优先、同类型取最新版本) 与系统常见位置.
//
// 隔离: 本脚本为确定性场景保留 --disable-features=BackForwardCache; L 场景
// 在 BFCache 不可用的前提下断言非 persisted pageshow 的快照恢复, 并在 facts
// 记录限制; 若该进程观察到 persisted pageshow 则改为断言恢复零重取.
//
// 场景:
//   A    陈旧 pane-nav 所有权: 在途分支点击被兑现 (恰一次请求), release 安装子项,
//        终态跟随最新意图翻转; 指针焦点落树根
//   A2   在途两次点击 (显式翻转): 恰一次请求, 终态收起; 第三次点击缓存命中展开
//   1    在途显式收起被更新导航覆盖: release 后祖先展开, 子项 current/选中, 恰一次请求
//   2    当前 nav 续接方在 release 前被取代: 陈旧续接零安装/零变异/零错误 UI
//   C    整页交接遗弃在途分支请求: 新文档模块状态重取并恰安装一次 (旧请求无落点)
//   3    跨文档 pending 身份: 在途再次激活复用同一 held 请求 (无第三次请求), 单次安装
//   D    已缓存折叠祖先 -> 面板下钻子项: 缓存命中展开, 子项 current/选中
//   E    重复持久化空目录: 恰一次请求, 不叶子化, branch-state=empty; 选项变化重取
//   Q17T 隐藏焦点回退 (树): 选中隐藏分支被隐藏 -> 回退最近可见祖先, 已打开内容保留;
//        再显示恢复展开且不抢焦点
//   Q17L 隐藏焦点回退 (列表): 同位置替换 hidden 查询, 回退最近可见行, 隐藏行不被打开
//   OH   仅含隐藏孩子的目录往返: empty 保持 expandable 且按选项版本重取; 环符号链接
//        保持不可展开 (零请求)
//   V    两个已展开分支一成一败: 成功装新数据, 失败显示 branch-state=error 安全错误;
//        旧版本响应零提交/零错误/零变异, 无自动重试
//   H    历史恢复失败: 保留目标 URL/快照, 面板错误态, 成功安装标记失效; 同路径再
//        恢复必重取并成功安装
//   L    reload + 跨整页后退/前进: 按快照恢复视图/规则/已打开条目/q 且不写偏好
//   R    展开子树后折叠父目录再刷新: 保持折叠/选中, 重新打开父目录恢复子树
//   R2   打开目录内文档后折叠祖先再刷新: 保留文档/URL/选中, 祖先保持折叠
//   R3   恢复子树不自动重试错误分支; 明确激活失败分支才允许重试

import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import net from 'node:net';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const results = [];
const scenarioFilter = process.env.CB_SCENARIO;
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
    // FETCH teardown: 先禁用 fetch 域 (释放全部被 held 的暂停请求 — 页面即将
    // 关闭, 任何失败路径都不得遗留 paused 请求/监听), 再移除监听并关闭目标.
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
  const root = mkdtempSync(join(tmpdir(), 'cb-tree-accept-fixture-'));
  writeFileSync(join(root, 'a.md'), '# A\n\npage a.\n');
  writeFileSync(join(root, '.hidden.md'), '# Hidden\n\nhidden page.\n');
  mkdirSync(join(root, 'adir', 'grand'), { recursive: true });
  writeFileSync(join(root, 'adir', 'child.md'), '# Child\n\nchild page.\n');
  writeFileSync(join(root, 'adir', 'grand', 'leaf.md'), '# Leaf\n\nleaf page.\n');
  mkdirSync(join(root, 'bdir'));
  writeFileSync(join(root, 'bdir', 'x.md'), '# X\n\nx page.\n');
  writeFileSync(join(root, 'bdir', '.bh.md'), '# BH\n\nbdir hidden child.\n');
  mkdirSync(join(root, 'emptydir'));
  // 仅含隐藏孩子的普通目录 (Q17 显示->隐藏->显示往返)
  mkdirSync(join(root, 'onlyhidden'));
  writeFileSync(join(root, 'onlyhidden', '.inside.md'), '# Inside\n\nonly hidden child.\n');
  // 混合目录: 隐藏子目录 + 隐藏文件 + 可见文件 (树焦点回退: 隐藏节点 -> 最近可见祖先)
  mkdirSync(join(root, 'mixdir', '.hiddendir'), { recursive: true });
  writeFileSync(join(root, 'mixdir', '.dot.md'), '# Dot\n\nhidden file.\n');
  writeFileSync(join(root, 'mixdir', 'visible.md'), '# Visible\n\nvisible file.\n');
  writeFileSync(join(root, 'mixdir', '.hiddendir', '.deep.md'), '# Deep\n\nhidden deep file.\n');
  // 循环符号链接: grand/loop -> .. (即 adir, 已访问目录) — 服务端标记
  // expandable=false, 客户端必须保持不可展开且零请求.
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

// 先等 Chrome 退出, 再删除 profile, 避免收尾写入重建临时目录.
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
        gone = true;
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
    // SIGTERM 后 Chrome 可能仍在收尾 (写 profile): 重试删除, 残留临时目录不
    // 阻断退出 (系统临时目录由 OS 清理).
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

const BIN = process.env.CB_BIN || join(import.meta.dirname, '..', 'target', 'release', 'chapbook');
if (!existsSync(BIN)) {
  throw new Error('missing ' + BIN + ' — run `cargo build --release` first');
}
const chromePath = findChrome();
if (!chromePath) {
  throw new Error('no Chrome found — set CHROME_BIN or install ms-playwright chromium');
}
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
const BASE = 'http://127.0.0.1:' + port;

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
    // 禁用 bfcache: 除 L 场景外, 场景间同 URL 导航必须产生全新文档与全新 init —
    // 上一场景的持久化状态若被 bfcache 恢复的旧文档内存态继承, 进入链持久化
    // walk 会预加载目录并污染场景请求计数. L 场景在此前提下断言非 persisted
    // pageshow 的恢复, 并把 BFCache 限制写进 facts.
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

const pausedCounts = {}; // tree path -> Fetch 暂停计数
const netCounts = {}; // tree path -> Network 请求计数 (含 query, 按 path 归并)
const fragCounts = {}; // fragment path -> Network 请求计数
// 当前场景活动页: fresh 创建后登记, runScenario 的 finally 统一关闭 — 任何
// 断言/步骤抛出时也执行 FETCH 释放 + 监听移除 + 页面关闭 (绝不遗留 paused
// 请求/监听/页面), 场景体不再各自 close.
let activePage = null;

// tree 请求: /__/tree[/{encoded}]?sort=Column:Order&hidden=0|1 (query 必须剥离
// 后再匹配 path; sort/hidden 逐请求捕获供按版本释放/过滤).
// fragment 请求: /{enc}?fragment=1[...] 与 /__/search?q=..&fragment=1[...].
function parseRequestUrl(url) {
  let u;
  try {
    u = new URL(url);
  } catch {
    return {};
  }
  const tree = u.pathname.match(/^\/__\/tree(?:\/(.*))?$/);
  if (tree) {
    return {
      tree: true,
      treePath: tree[1] ? '/' + tree[1] : '/',
      sort: u.searchParams.get('sort'),
      hidden: u.searchParams.get('hidden'),
    };
  }
  if (u.searchParams.get('fragment') === '1') {
    const p = u.pathname.replace(/\/+$/, '');
    return { fragment: true, fragPath: p === '' ? '/' : p };
  }
  return {};
}

async function waitClientReady(page) {
  await waitFor(
    () =>
      page.evaluate(
        "(() => { const reading = document.querySelector('[data-cb-reading-tree]'); if (!document.body.classList.contains('cb-tree-mode') && reading) return !reading.hidden; const b = document.querySelector('[data-cb-view=\"tree\"]'); return !!b && b.getAttribute('aria-disabled') === null; })()"
      ),
    'client ready',
    10000
  );
}

// 进入树视图: 浏览页 -> 树为 SPA 视图切换 (同位置 replace, 无整页重载).
async function enterTree(page) {
  await waitClientReady(page);
  await page.click('[data-cb-view="tree"]');
  await waitFor(
    () => page.evaluate("!!document.querySelector('ul#cb-tree-root > li.cb-tree-node')"),
    'tree root attached',
    10000
  );
  await sleep(300); // 根 JSON + 初始内容区 fragment settle
}

async function fresh(mode, opts) {
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
  const onRequest = (params, sid) => {
    if (sid !== sessionId) return;
    const info = parseRequestUrl(params.request.url);
    if (info.tree) {
      netCounts[info.treePath] = (netCounts[info.treePath] || 0) + 1;
    } else if (info.fragment) {
      fragCounts[info.fragPath] = (fragCounts[info.fragPath] || 0) + 1;
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
  // 场景写入的偏好/展开记录若被进入链读取, 会污染场景请求计数 — 首个 goto 即
  // 全新文档全新 init, 绝不依赖 clear+reload 的时序.
  await page.send('Storage.clearDataForOrigin', {
    origin: BASE,
    storageTypes: 'local_storage',
  });
  await page.goto(BASE + '/');
  if (opts && opts.patchAbort) {
    // 传输层中止伪装 no-op: 版本化判定的验收场景需要旧版本在途请求真实
    // resolve — abort 被伪装后旧响应才能释放, 守卫 (版本/身份) 才被真正检验.
    await page.evaluate(
      "(() => { window.__abortMock = 0; const OrigAC = AbortController; window.AbortController = function () { const ac = new OrigAC(); ac.abort = function () { window.__abortMock++; }; return ac; }; })()"
    );
  }
  if (mode === 'tree') {
    await enterTree(page);
  } else {
    await waitClientReady(page);
  }
  // 每场景独立计数 (模块级表不得跨场景泄漏)
  Object.keys(pausedCounts).forEach((k) => delete pausedCounts[k]);
  Object.keys(netCounts).forEach((k) => delete netCounts[k]);
  Object.keys(fragCounts).forEach((k) => delete fragCounts[k]);
  return { page, pageErrors };
}

// holdTreePath: 暂停 holdSet 内 tree 路径的全部请求 (held: path -> 逐请求记录
// {requestId, sort, hidden}); failOnce: Map<path, count> 先以 500 兑现请求
// (用于单分支当前版本失败注入); 其余请求放行. 全部 held 请求由任何失败路径上
// 的 Page.close -> Fetch.disable 兜底释放, 绝不遗留.
function holdTreePath(page, holdSet, held, opts) {
  const failOnce = (opts && opts.failOnce) || new Map();
  const handler = (params, sid) => {
    if (sid !== page.sessionId) return;
    const info = parseRequestUrl(params.request.url);
    const path = info.tree ? info.treePath
      : opts && opts.fragments && info.fragment ? 'fragment:' + info.fragPath : null;
    if (path === null) {
      page.send('Fetch.continueRequest', { requestId: params.requestId }).catch(() => {});
      return;
    }
    pausedCounts[path] = (pausedCounts[path] || 0) + 1;
    const remaining = failOnce.get(path) || 0;
    if (remaining > 0) {
      failOnce.set(path, remaining - 1);
      page.send('Fetch.fulfillRequest', {
        requestId: params.requestId,
        responseCode: 500,
        responseHeaders: [{ name: 'Content-Type', value: 'application/json' }],
        body: Buffer.from('{"error":"injected"}').toString('base64'),
      }).catch(() => {});
      return;
    }
    if (holdSet.has(path)) {
      const list = held.get(path) || [];
      list.push({ requestId: params.requestId, sort: info.sort, hidden: info.hidden });
      held.set(path, list);
    } else {
      page.send('Fetch.continueRequest', { requestId: params.requestId }).catch(() => {});
    }
  };
  page.on('Fetch.requestPaused', handler);
  return page.send('Fetch.enable', {
    patterns: opts && opts.fragments
      ? [{ urlPattern: '*__/tree*' }, { urlPattern: '*fragment=1*' }]
      : [{ urlPattern: '*__/tree*' }],
    requestStage: 'Request',
  });
}

// 下一次命中 path 的 fragment 请求以 500 兑现 (其余 fragment 放行); 兑现后
// 该 path 的正常请求恢复放行 — 历史恢复失败 + 同路径重取场景的故障注入.
function failNextFragment(page, state) {
  const handler = (params, sid) => {
    if (sid !== page.sessionId) return;
    const info = parseRequestUrl(params.request.url);
    if (info.fragment && info.fragPath === state.path && state.remaining > 0) {
      state.remaining -= 1;
      state.failed += 1;
      page.send('Fetch.fulfillRequest', {
        requestId: params.requestId,
        responseCode: 500,
        responseHeaders: [{ name: 'Content-Type', value: 'text/plain' }],
        body: Buffer.from('injected failure').toString('base64'),
      }).catch(() => {});
      return;
    }
    page.send('Fetch.continueRequest', { requestId: params.requestId }).catch(() => {});
  };
  page.on('Fetch.requestPaused', handler);
  return page.send('Fetch.enable', {
    patterns: [{ urlPattern: '*fragment=1*' }],
    requestStage: 'Request',
  });
}

// 释放一个 held 请求: opts 可按 {sort, hidden} 过滤, fail 注入 500;
// tolerateError 用于旧文档销毁后浏览器已丢弃请求的场景, 错误记录在返回 facts.
async function releaseOne(page, held, path, opts) {
  const list = held.get(path) || [];
  const idx = list.findIndex(
    (v) =>
      !opts ||
      ((opts.sort === undefined || v.sort === opts.sort) &&
        (opts.hidden === undefined || v.hidden === opts.hidden))
  );
  if (idx < 0) return null;
  const entry = list.splice(idx, 1)[0];
  if (list.length === 0) held.delete(path);
  try {
    if (opts && opts.fail) {
      await page.send('Fetch.fulfillRequest', {
        requestId: entry.requestId,
        responseCode: 500,
        responseHeaders: [{ name: 'Content-Type', value: 'application/json' }],
        body: Buffer.from('{"error":"injected-stale"}').toString('base64'),
      });
    } else {
      await page.send('Fetch.continueRequest', { requestId: entry.requestId });
    }
    return entry;
  } catch (err) {
    if (opts && opts.tolerateError) {
      entry.error = err.message;
      return entry;
    }
    throw new Error('release failed: ' + err.message);
  }
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

/* ---------- 状态读取器 (断言只观察可访问/公共 DOM 契约) ---------- */

const dirState = (page, path) =>
  page.evaluate(
    '(() => { const li = document.querySelector(' + JSON.stringify('[data-native-path-encoded="' + path + '"]') + '); const ul = li ? li.querySelector(":scope > ul.cb-tree-children") : null; const nodes = ul ? Array.from(ul.querySelectorAll(":scope > li.cb-tree-node")) : []; const err = li ? li.querySelector(".cb-tree-branch-error[role=\\"alert\\"]") : null; const twist = li ? li.querySelector(":scope > .cb-tree-row .cb-tree-twist") : null; return { expandable: li ? li.dataset.treeExpandable : null, expanded: li ? li.getAttribute("aria-expanded") : null, twist: twist ? twist.textContent : null, hasUl: !!ul, hidden: ul ? ul.hidden : null, branchState: li ? li.getAttribute("data-cb-branch-state") : null, children: nodes.length, names: nodes.map((n) => { const b = n.querySelector("bdi"); return b ? b.textContent : null; }), errorAlert: !!err, errorText: err ? err.textContent : null }; })()'
  );

const expandedList = (page) =>
  page.evaluate("JSON.parse(localStorage.getItem('cb-tree-expanded') || 'null')");

const currentPaths = (page) =>
  page.evaluate(
    "Array.from(document.querySelectorAll('#cb-tree-root .cb-current')).map((el) => el.dataset.nativePathEncoded)"
  );

const selState = (page) =>
  page.evaluate(
    '(() => { const root = document.getElementById("cb-tree-root"); const ad = root ? root.getAttribute("aria-activedescendant") : null; const sel = document.querySelector("#cb-tree-root .cb-tree-node.cb-selected"); return { selected: sel ? sel.dataset.nativePathEncoded : null, ad, adLive: ad ? !!document.getElementById(ad) : false, selTrue: document.querySelectorAll("#cb-tree-root [aria-selected=\\"true\\"]").length, activeId: document.activeElement ? document.activeElement.id : null }; })()'
  );

const paneState = (page) =>
  page.evaluate(
    '(() => { const pane = document.querySelector(".cb-pane"); const err = pane ? pane.querySelector(":scope > .cb-pane-error") : null; const frag = pane ? pane.querySelector(":scope > .cb-fragment, :scope > .cb-doc") : null; return { exists: !!pane, state: pane ? pane.getAttribute("data-cb-pane-state") : null, error: !!(err && err.getAttribute("role") === "alert"), errorText: err ? err.textContent : null, content: frag ? frag.getAttribute("data-native-path-encoded") : null, surface: frag ? frag.getAttribute("data-cb-surface") : null, miniLinks: pane ? pane.querySelectorAll(".cb-dir-mini a[data-native-path-encoded]").length : -1 }; })()'
  );

const listState = (page) =>
  page.evaluate(
    '(() => { const host = document.getElementById("cb-view-list"); const grid = document.getElementById("cb-view-grid"); const rows = host ? Array.from(host.querySelectorAll("[data-browser-entry]")) : []; const selected = host ? Array.from(host.querySelectorAll("[data-browser-entry].cb-selected")) : []; const pressed = {}; document.querySelectorAll("[data-cb-view]").forEach((b) => { pressed[b.getAttribute("data-cb-view")] = b.getAttribute("aria-pressed"); }); const toggle = document.querySelector("[data-cb-hidden-toggle]"); return { treeMode: document.body.classList.contains("cb-tree-mode"), listHidden: host ? host.hidden : null, gridHidden: grid ? grid.hidden : null, rows: rows.map((r) => r.dataset.nativePathEncoded), selected: selected.map((r) => r.dataset.nativePathEncoded), pressed, hiddenPressed: toggle ? toggle.getAttribute("aria-pressed") : null, pathname: location.pathname, search: location.search, hash: location.hash, query: document.body.dataset.cbQuery || null }; })()'
  );

const urlRules = (page) =>
  page.evaluate(
    '(() => { const p = new URLSearchParams(location.search); return { sort: p.get("sort"), hidden: p.get("hidden"), q: p.get("q"), pathname: location.pathname, hash: location.hash }; })()'
  );

// 键盘选取: 行点击会在分支安装后清空选中 (安装替换子树), 因此 "选中隐藏节点
// 再隐藏" 的 Q17 场景经 ArrowDown 行走可见节点完成 — 真实键盘路径, 且选中
// 不依赖任何内部状态. 返回是否到达 target.
async function selectTreeNodeByKeyboard(page, target, max = 24) {
  await page.evaluate("document.getElementById('cb-tree-root').focus()");
  for (let i = 0; i < max; i++) {
    const cur = (await selState(page)).selected;
    if (cur === target) return true;
    await page.press('ArrowDown', 'ArrowDown', 40);
    await sleep(60);
  }
  return (await selState(page)).selected === target;
}

const prefsState = (page) =>
  page.evaluate(
    '(() => { const get = (k) => { try { return localStorage.getItem(k); } catch (err) { return "ERR"; } }; return { view: get("cb-browse-view"), sort: get("cb-sort"), hidden: get("cb-hidden"), legacyMode: get("cb-mode"), legacyView: get("cb-view") }; })()'
  );

const historyLen = (page) => page.evaluate('history.length');

// 另一个 tab 改共享偏好 (跨 tab 场景): 真实第二 target 写入后关闭; 返回写入值.
async function setOtherTabPrefs(entries) {
  const { targetId } = await browserCdp.send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await browserCdp.send('Target.attachToTarget', { targetId, flatten: true });
  const tab = new Page(browserCdp, targetId, sessionId);
  try {
    await tab.goto(BASE + '/');
    await tab.evaluate(
      '(() => { const prefs = ' + JSON.stringify(entries) + '; Object.keys(prefs).forEach((k) => localStorage.setItem(k, prefs[k])); return true; })()'
    );
    return await tab.evaluate(
      '(() => ({ view: localStorage.getItem("cb-browse-view"), sort: localStorage.getItem("cb-sort"), hidden: localStorage.getItem("cb-hidden") }))()'
    );
  } finally {
    await tab.close();
  }
}

async function runScenario(name, fn) {
  if (scenarioFilter && name !== scenarioFilter) return;
  try {
    await fn();
  } catch (e) {
    const state = activePage
      ? await activePage.evaluate(`(() => ({
          snapshot: history.state && history.state.cb,
          hiddenPressed: document.querySelector('[data-cb-hidden-toggle]')?.getAttribute('aria-pressed'),
          expanded: localStorage.getItem('cb-tree-expanded'),
          rootBusy: document.querySelector('#cb-tree-root')?.getAttribute('aria-busy'),
          nodes: [...document.querySelectorAll('#cb-tree-root li.cb-tree-node')].map((li) => ({
            path: li.dataset.nativePathEncoded,
            expanded: li.getAttribute('aria-expanded'),
            state: li.getAttribute('data-cb-branch-state'),
            children: li.querySelectorAll(':scope > ul.cb-tree-children > li.cb-tree-node').length
          }))
        }))()`).catch(() => null)
      : null;
    rec(name, false, 'THREW: ' + String(e && e.message ? e.message : e) +
      ' state=' + JSON.stringify(state) + ' requests=' + JSON.stringify(netCounts));
  } finally {
    // 异常安全收尾: 任何失败路径都释放 FETCH/监听并关闭页面 (幂等 — 场景体
    // 不再各自 close), 随后继续下一场景/汇总.
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

/* =====================================================================
 * R. 点击展开/折叠同一目录再刷新: 选中目录不等于要求展开该目录.
 * ===================================================================== */
await runScenario('R explicit collapse survives reload', async () => {
  const { page, pageErrors } = await fresh('tree');
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await waitFor(() => page.evaluate(
    "document.querySelector('#cb-tree-root [data-native-path-encoded=\"adir\"]')?.getAttribute('aria-expanded') === 'true' && !!document.querySelector('#cb-tree-root [data-native-path-encoded=\"adir/child.md\"]')"
  ), 'expanded-directory');
  await page.click('[data-native-path-encoded="adir/grand"] .cb-tree-row');
  await waitFor(() => page.evaluate(
    "document.querySelector('#cb-tree-root [data-native-path-encoded=\"adir/grand\"]')?.getAttribute('aria-expanded') === 'true' && !!document.querySelector('#cb-tree-root [data-native-path-encoded=\"adir/grand/leaf.md\"]')"
  ), 'expanded-child-directory');
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  const collapsed = await dirState(page, 'adir');
  const selected = await selState(page);
  const urlBefore = await urlRules(page);
  const lengthBefore = await historyLen(page);
  const persistedBefore = await expandedList(page);

  await page.send('Page.setLifecycleEventsEnabled', { enabled: true });
  const { frameTree } = await page.send('Page.getFrameTree');
  let reloadIdle = false;
  page.on('Page.lifecycleEvent', (event, sid) => {
    if (sid === page.sessionId && event.loaderId !== frameTree.frame.loaderId && event.name === 'networkIdle') reloadIdle = true;
  });
  await page.evaluate('window.__cbCollapseReloadMarker = true');
  await page.send('Page.reload');
  await waitFor(() => page.evaluate(
    "typeof window.__cbCollapseReloadMarker === 'undefined' && !!document.querySelector('#cb-tree-root [data-native-path-encoded=\"adir\"]')"
  ), 'new-tree-document');
  await waitFor(() => reloadIdle, 'reloaded-tree-network-idle');
  const restored = await dirState(page, 'adir');
  const restoredSelection = await selState(page);
  const urlAfter = await urlRules(page);
  const lengthAfter = await historyLen(page);
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await waitFor(() => page.evaluate(
    "!!document.querySelector('#cb-tree-root [data-native-path-encoded=\"adir/grand/leaf.md\"]')"
  ), 'remembered-subtree-after-reopening');
  const reopened = await dirState(page, 'adir');
  const remembered = await dirState(page, 'adir/grand');
  rec('R explicit collapse survives reload',
    collapsed.expanded === 'false' && collapsed.hidden === true &&
      selected.selected === 'adir' && restored.expanded === 'false' && restored.hidden !== false &&
      restoredSelection.selected === 'adir' && restoredSelection.adLive === true &&
      reopened.expanded === 'true' && reopened.hidden === false &&
      remembered.expanded === 'true' && remembered.hidden === false &&
      JSON.stringify(urlAfter) === JSON.stringify(urlBefore) && lengthAfter === lengthBefore &&
      pageErrors.length === 0,
    JSON.stringify({ collapsed, selected, persistedBefore, restored, restoredSelection, reopened, remembered, urlBefore, urlAfter, lengthBefore, lengthAfter, errors: pageErrors }));
});

/* =====================================================================
 * R2. 打开目录内文档后折叠祖先再刷新: 文档保留, 祖先不因内容定位重新展开.
 * ===================================================================== */
await runScenario('R2 opened document keeps manually collapsed ancestor on reload', async () => {
  const { page, pageErrors } = await fresh('tree');
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await waitFor(() => page.evaluate(
    "!!document.querySelector('#cb-tree-root [data-native-path-encoded=\"adir/child.md\"]')"
  ), 'document-tree-entry');
  await page.click('#cb-tree-root [data-native-path-encoded="adir/child.md"] .cb-tree-row');
  await waitFor(async () => {
    const pane = await paneState(page);
    return pane.state === 'ready' && pane.content === 'adir/child.md';
  }, 'opened-child-document');
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  const collapsed = await dirState(page, 'adir');
  const urlBefore = await urlRules(page);
  const lengthBefore = await historyLen(page);
  const textBefore = await page.evaluate("document.querySelector('.cb-pane .cb-doc').innerText");

  await page.send('Page.setLifecycleEventsEnabled', { enabled: true });
  const { frameTree } = await page.send('Page.getFrameTree');
  let reloadIdle = false;
  page.on('Page.lifecycleEvent', (event, sid) => {
    if (sid === page.sessionId && event.loaderId !== frameTree.frame.loaderId && event.name === 'networkIdle') reloadIdle = true;
  });
  await page.evaluate('window.__cbCollapseDocumentMarker = true');
  await page.send('Page.reload');
  await waitFor(() => page.evaluate(
    "typeof window.__cbCollapseDocumentMarker === 'undefined' && !!document.querySelector('#cb-tree-root [data-native-path-encoded=\"adir\"]')"
  ), 'new-document-tree');
  await waitFor(() => reloadIdle, 'document-tree-network-idle');
  const restored = await dirState(page, 'adir');
  const selected = await selState(page);
  const pane = await paneState(page);
  const urlAfter = await urlRules(page);
  const lengthAfter = await historyLen(page);
  const textAfter = await page.evaluate("document.querySelector('.cb-pane .cb-doc').innerText");
  rec('R2 opened document keeps manually collapsed ancestor on reload',
    collapsed.expanded === 'false' && collapsed.hidden === true &&
      restored.expanded === 'false' && restored.hidden !== false &&
      selected.selected === 'adir' && selected.adLive === true &&
      pane.state === 'ready' && pane.content === 'adir/child.md' && textAfter === textBefore &&
      JSON.stringify(urlAfter) === JSON.stringify(urlBefore) && lengthAfter === lengthBefore &&
      pageErrors.length === 0,
    JSON.stringify({ collapsed, restored, selected, pane, urlBefore, urlAfter, lengthBefore, lengthAfter, textBefore, textAfter, errors: pageErrors }));
});

/* =====================================================================
 * R3. 恢复子树展开不是自动重试失败分支; 明确激活该分支才允许重试.
 * ===================================================================== */
await runScenario('R3 reopening a parent preserves child error until explicit retry', async () => {
  const { page, pageErrors } = await fresh('tree');
  const holdSet = new Set();
  const held = new Map();
  await holdTreePath(page, holdSet, held, { failOnce: new Map([['/adir/grand', 1]]) });
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await waitFor(() => page.evaluate(
    "!!document.querySelector('#cb-tree-root [data-native-path-encoded=\"adir/grand\"]')"
  ), 'retry-child-directory');
  await page.click('[data-native-path-encoded="adir/grand"] .cb-tree-row');
  await waitFor(() => page.evaluate(
    "document.querySelector('#cb-tree-root [data-native-path-encoded=\"adir/grand\"]')?.getAttribute('data-cb-branch-state') === 'error'"
  ), 'child-directory-error');
  holdSet.add('/adir/grand'); // 意外自动重试会保持 loading, 不能被快速成功掩盖.
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  const retained = await dirState(page, 'adir/grand');
  if (retained.branchState !== 'error' || !retained.errorAlert) {
    rec('R3 reopening a parent preserves child error until explicit retry', false, JSON.stringify({ retained, errors: pageErrors }));
    return;
  }
  holdSet.delete('/adir/grand');
  await page.click('[data-native-path-encoded="adir/grand"] .cb-tree-row');
  await page.click('[data-native-path-encoded="adir/grand"] .cb-tree-row');
  await waitFor(() => page.evaluate(
    "!!document.querySelector('#cb-tree-root [data-native-path-encoded=\"adir/grand/leaf.md\"]')"
  ), 'explicit-child-retry-success');
  const retried = await dirState(page, 'adir/grand');
  rec('R3 reopening a parent preserves child error until explicit retry',
    retried.expanded === 'true' && retried.hidden === false && retried.branchState === null &&
      !retried.errorAlert && pageErrors.length === 0,
    JSON.stringify({ retained, retried, errors: pageErrors }));
});

/* =====================================================================
 * A. 陈旧 pane-nav 所有权: 面板迷你列表 -> /adir 发起分支加载 (held, nav
 *    所有权); 随后树内导航 a.md 使该 nav 陈旧; 用户在途点击 loading 分支续接:
 *    release 后恰一次请求, 子项安装, 终态跟随最新意图 (点击翻转), 指针焦点
 *    落树根.
 * ===================================================================== */
await runScenario('A stale pane-nav owner: click on loading branch adopts one request; release installs children honoring the flip; focus on tree root', async () => {
  const { page, pageErrors } = await fresh('tree');
  const holdSet = new Set(['/adir']);
  const held = new Map();
  await holdTreePath(page, holdSet, held);

  // nav A: 面板迷你列表 -> /adir -> 按 nav 所有权加载 adir 分支 (held)
  await waitFor(
    () => page.evaluate("!!document.querySelector('.cb-pane .cb-dir-mini a[data-native-path-encoded=\"adir\"]')"),
    'pane-mini-ready'
  );
  await page.click('.cb-pane .cb-dir-mini a[data-native-path-encoded="adir"]');
  await waitHeld(page, held, '/adir', 1, 'A-adir-held');
  await sleep(300);
  // nav B: 树行点击 a.md -> SPA 导航 (nav A 陈旧; adir 分支仍在途)
  await page.click('[data-native-path-encoded="a.md"] .cb-tree-row');
  await waitFor(() => page.evaluate("location.pathname === '/a.md'"), 'commit-B');
  await sleep(300);
  const preClick = await dirState(page, 'adir');
  // 用户点击在途 (loading) 分支: 续接, 不发起第二次请求; 最新意图翻转
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await sleep(400);
  const focusState = await page.evaluate(
    '(() => ({ activeId: document.activeElement ? document.activeElement.id : null, activeTag: document.activeElement ? document.activeElement.tagName : null }))()'
  );
  const mid = { held: (held.get('/adir') || []).length, paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  // release: 恰一次请求; 续接方安装子项, 终态跟随最新意图 (点击表达收起)
  await releaseOne(page, held, '/adir');
  await waitFor(
    () => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"adir\"] ul.cb-tree-children li.cb-tree-node')"),
    'adir-landed'
  );
  await sleep(400);
  const final = await dirState(page, 'adir');
  const expanded = await expandedList(page);
  const cur = await currentPaths(page);
  const sel = await selState(page);
  const counts = { paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  rec('A stale pane-nav owner: click on loading branch adopts one request; release installs children honoring the flip; focus on tree root',
    preClick.expandable === '1' && preClick.children === 0 && preClick.errorAlert === false &&
      preClick.branchState !== 'error' &&
      focusState.activeId === 'cb-tree-root' &&
      mid.held === 1 && mid.paused === 1 && mid.net === 1 &&
      final.children === 2 && final.hidden === true && final.expanded === 'false' &&
      !(JSON.stringify(expanded) || '').includes('adir') &&
      JSON.stringify(cur) === JSON.stringify(['a.md']) &&
      sel.selected === 'adir' && sel.adLive === true &&
      counts.paused === 1 && counts.net === 1 && pageErrors.length === 0,
    'pre=' + JSON.stringify(preClick) + ' focus=' + JSON.stringify(focusState) + ' mid=' + JSON.stringify(mid) +
      ' final=' + JSON.stringify(final) + ' expanded=' + JSON.stringify(expanded) + ' current=' + JSON.stringify(cur) +
      ' sel=' + JSON.stringify(sel) + ' counts=' + JSON.stringify(counts) + ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * A2. 在途两次点击 (显式翻转): 第二次激活表达与上次相反的终态 — release 后
 *     落地收起 (子项已安装), 仍恰一次请求; 第三次点击缓存命中展开且零额外
 *     请求 (每次点击都被兑现).
 * ===================================================================== */
await runScenario('A2 two clicks while pending: exactly one request; release honors collapse intent; third click expands from cache', async () => {
  const { page, pageErrors } = await fresh('tree');
  const holdSet = new Set(['/adir']);
  const held = new Map();
  await holdTreePath(page, holdSet, held);

  // 点击1 (无在途): 展开并发起加载 (held); 点击2 在途翻转 -> 收起意图
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await waitHeld(page, held, '/adir', 1, 'A2-adir-held');
  await sleep(250);
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await sleep(400);
  const mid = { held: (held.get('/adir') || []).length, paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  await releaseOne(page, held, '/adir');
  await waitFor(
    () => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"adir\"] ul.cb-tree-children li.cb-tree-node')"),
    'A2-adir-landed'
  );
  await sleep(400);
  const final = await dirState(page, 'adir');
  const expanded = await expandedList(page);
  const counts = { paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  // 第三次点击 (缓存命中, 在途已决): 展开 — 每次点击都被兑现, 零额外请求
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await sleep(400);
  const third = await dirState(page, 'adir');
  const counts2 = { paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  rec('A2 two clicks while pending: exactly one request; release honors collapse intent; third click expands from cache',
    mid.held === 1 && mid.paused === 1 && mid.net === 1 &&
      final.children === 2 && final.hidden === true && final.expanded === 'false' &&
      !(JSON.stringify(expanded) || '').includes('adir') &&
      third.hidden === false && third.expanded === 'true' && third.children === 2 &&
      counts.paused === 1 && counts.net === 1 && counts2.paused === 1 && counts2.net === 1 &&
      pageErrors.length === 0,
    'mid=' + JSON.stringify(mid) + ' final=' + JSON.stringify(final) + ' expanded=' + JSON.stringify(expanded) +
      ' third=' + JSON.stringify(third) + ' counts=' + JSON.stringify(counts) + ' counts2=' + JSON.stringify(counts2) +
      ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * 1. 在途显式收起被更新导航覆盖 (版本化意图): nav A 发起 P 请求 (在途);
 *    在途期间显式点击两次表达收起; 更新的导航 C (back 回 P) 与 D (下钻
 *    P/child) 复用同一请求并发布更新意图 — release 后安装方展开, 子项
 *    current/选中, 恰一次请求.
 * ===================================================================== */
await runScenario('1 newer nav while pending overrides explicit collapse: release -> ancestor expanded, child current/selected, one request', async () => {
  const { page, pageErrors } = await fresh('tree');
  const holdSet = new Set(['/adir']);
  const held = new Map();
  await holdTreePath(page, holdSet, held);

  // nav A: 面板 -> /adir (held; nav 默认意图展开)
  await waitFor(
    () => page.evaluate("!!document.querySelector('.cb-pane .cb-dir-mini a[data-native-path-encoded=\"adir\"]')"),
    'pane-mini-ready'
  );
  await page.click('.cb-pane .cb-dir-mini a[data-native-path-encoded="adir"]');
  await waitHeld(page, held, '/adir', 1, '1-adir-held');
  await sleep(300);
  // nav B: 树行点击 a.md -> 提交 (nav A 陈旧; adir 仍在途)
  await page.click('[data-native-path-encoded="a.md"] .cb-tree-row');
  await waitFor(() => page.evaluate("location.pathname === '/a.md'"), '1-commit-B');
  await sleep(300);
  // 在途期间显式点击两次: 点击1 = 展开意图, 点击2 = 翻转 -> 收起
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await sleep(250);
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await sleep(300);
  // nav C: back -> /adir (popstate 导航; 复用同一在途请求)
  await page.evaluate('history.back()');
  await waitFor(() => page.evaluate("location.pathname === '/adir'"), '1-nav-C-adir');
  // nav D: 面板下钻 P/child (再发布更新意图)
  await waitFor(
    () => page.evaluate("!!document.querySelector('.cb-pane .cb-dir-mini a[data-native-path-encoded=\"adir/child.md\"]')"),
    '1-pane-child-mini-ready'
  );
  await page.click('.cb-pane .cb-dir-mini a[data-native-path-encoded="adir/child.md"]');
  await waitFor(() => page.evaluate("location.pathname === '/adir/child.md'"), '1-nav-D-child');
  await sleep(300);
  const mid = { held: (held.get('/adir') || []).length, paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  // release: 恰一次请求; 更新导航的安装方展开 (覆盖更早的显式收起意图)
  await releaseOne(page, held, '/adir');
  await waitFor(
    () => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"adir/child.md\"]')"),
    '1-child-landed'
  );
  await sleep(400);
  const final = await dirState(page, 'adir');
  const expanded = await expandedList(page);
  const cur = await currentPaths(page);
  const sel = await selState(page);
  const counts = { paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  rec('1 newer nav while pending overrides explicit collapse: release -> ancestor expanded, child current/selected, one request',
    mid.held === 1 && mid.paused === 1 && mid.net === 1 &&
      final.children === 2 && final.hidden === false && final.expanded === 'true' &&
      Array.isArray(expanded) && expanded.includes('adir') &&
      JSON.stringify(cur) === JSON.stringify(['adir', 'adir/child.md']) &&
      sel.selected === 'adir/child.md' && sel.adLive === true &&
      counts.paused === 1 && counts.net === 1 && pageErrors.length === 0,
    'mid=' + JSON.stringify(mid) + ' final=' + JSON.stringify(final) + ' expanded=' + JSON.stringify(expanded) +
      ' current=' + JSON.stringify(cur) + ' sel=' + JSON.stringify(sel) + ' counts=' + JSON.stringify(counts) +
      ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * 2. 当前导航续接方在 release 前被更新导航取代: nav A 发起 P (在途);
 *    nav B 提交别处 (A 陈旧); nav C 回到 P (续接方挂载时 C 当前);
 *    nav R (Backspace -> 根) 再更新导航 (C 陈旧); release 后陈旧响应与
 *    陈旧续接都不得安装/变异 DOM、持久化意图、current/选中, 零错误 UI.
 * ===================================================================== */
await runScenario('2 current nav adopter superseded before release: stale adopter cannot install/mutate', async () => {
  const { page, pageErrors } = await fresh('tree');
  const holdSet = new Set(['/adir']);
  const held = new Map();
  await holdTreePath(page, holdSet, held);

  // nav A: 面板 -> /adir (held)
  await waitFor(
    () => page.evaluate("!!document.querySelector('.cb-pane .cb-dir-mini a[data-native-path-encoded=\"adir\"]')"),
    'pane-mini-ready'
  );
  await page.click('.cb-pane .cb-dir-mini a[data-native-path-encoded="adir"]');
  await waitHeld(page, held, '/adir', 1, '2-adir-held');
  await sleep(300);
  // nav B: 树行点击 a.md -> 提交 (nav A 陈旧)
  await page.click('[data-native-path-encoded="a.md"] .cb-tree-row');
  await waitFor(() => page.evaluate("location.pathname === '/a.md'"), '2-commit-B');
  await sleep(300);
  // nav C: back -> /adir (popstate; 续接方挂载时 C 当前, 复用同一在途请求)
  await page.evaluate('history.back()');
  await waitFor(() => page.evaluate("location.pathname === '/adir'"), '2-nav-C');
  await sleep(300);
  // nav R: Backspace -> 根 (更新导航取代 C)
  await page.press('Backspace', 'Backspace', 8);
  await waitFor(() => page.evaluate("location.pathname === '/'"), '2-nav-R');
  await sleep(300);
  const mid = { held: (held.get('/adir') || []).length, paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  // 释放前快照: 发起方导航已把该分支置入当前壳的 loading 状态 (不是陈旧变异)
  const preRelease = {
    adir: await dirState(page, 'adir'),
    expanded: await expandedList(page),
    current: await currentPaths(page),
    sel: await selState(page),
  };
  // release A: 陈旧响应只共享缓存, C 的续接 settle 时已非当前 — 零安装/零变异
  await releaseOne(page, held, '/adir');
  await sleep(700);
  const s = {
    adir: await dirState(page, 'adir'),
    expanded: await expandedList(page),
    current: await currentPaths(page),
    sel: await selState(page),
    branchErrors: await page.evaluate("document.querySelectorAll('.cb-tree-branch-error').length"),
    paneError: await page.evaluate("!!document.querySelector('.cb-pane-error')"),
    rootChildren: await page.evaluate("document.querySelectorAll('#cb-tree-root > li.cb-tree-node').length"),
  };
  rec('2 current nav adopter superseded before release: stale adopter cannot install/mutate',
    mid.held === 1 && mid.paused === 1 && mid.net === 1 &&
      s.adir.children === 0 && s.adir.errorAlert === false && s.adir.branchState !== 'error' &&
      JSON.stringify({ adir: s.adir, expanded: s.expanded, current: s.current, sel: s.sel }) === JSON.stringify(preRelease) &&
      !(JSON.stringify(s.expanded) || '').includes('adir') &&
      JSON.stringify(s.current) === JSON.stringify([]) &&
      s.sel.selected === null && s.sel.selTrue === 0 &&
      s.branchErrors === 0 && s.paneError === false && s.rootChildren === 6 &&
      pageErrors.length === 0,
    'mid=' + JSON.stringify(mid) + ' preRelease=' + JSON.stringify(preRelease) + ' s=' + JSON.stringify(s) +
      ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * C. 整页交接遗弃在途分支请求: 文档 1 发起 P (held); 离树 (tree -> list
 *    同路径 replace + reload, 历史长度不变) 销毁旧文档; 文档 2 重新入树
 *    并加载 P — 旧文档的暂停请求被释放 (可能已被浏览器丢弃) 不得影响新
 *    模块状态; 新文档恰安装一次, 无第三次请求.
 * ===================================================================== */
await runScenario('C full-page handoff abandons in-flight branch: fresh document installs its own branch exactly once', async () => {
  const { page, pageErrors } = await fresh('tree');
  const holdSet = new Set(['/adir']);
  const held = new Map();
  await holdTreePath(page, holdSet, held);

  // 文档 1: 点击 adir -> 请求 A held
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await waitHeld(page, held, '/adir', 1, 'C-doc1-held');
  await waitFor(() => (netCounts['/adir'] || 0) === 1, 'C-doc1-request-observed');
  const doc1 = { paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  const lenBefore = await historyLen(page);
  // 离树: tree -> list 整页交接 (同路径 replace + reload)
  await page.click('[data-cb-view="list"]');
  await waitFor(
    () => page.evaluate("!document.querySelector('ul#cb-tree-root') && location.pathname === '/'"),
    'C-doc2-list'
  );
  const lenAfter = await historyLen(page);
  // 文档 2: 重新入树 (全新模块状态); 进入链按持久化展开意图加载 adir (held),
  // 恰一次请求 — 旧文档的请求与它无关
  await enterTree(page);
  await waitHeld(page, held, '/adir', 2, 'C-doc2-held');
  const mid = { held: (held.get('/adir') || []).length, paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  const preStray = await dirState(page, 'adir');
  // 释放旧文档的暂停请求: 文档已销毁 -> 无落点 (CDP 可能报 invalid interception)
  const stray = await releaseOne(page, held, '/adir', { tolerateError: true });
  await sleep(600);
  const afterStray = {
    held: (held.get('/adir') || []).length,
    paused: pausedCounts['/adir'] || 0,
    net: netCounts['/adir'] || 0,
    state: await dirState(page, 'adir'),
    branchErrors: await page.evaluate("document.querySelectorAll('.cb-tree-branch-error').length"),
  };
  // release B: 文档 2 恰安装一次
  await releaseOne(page, held, '/adir');
  await waitFor(
    () => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"adir\"] ul.cb-tree-children li.cb-tree-node')"),
    'C-doc2-landed'
  );
  await sleep(300);
  const finalB = await dirState(page, 'adir');
  const counts = { held: (held.get('/adir') || []).length, paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  rec('C full-page handoff abandons in-flight branch: fresh document installs its own branch exactly once',
    doc1.paused === 1 && doc1.net === 1 &&
      lenAfter === lenBefore &&
      mid.held === 2 && mid.paused === 2 && mid.net === 2 &&
      stray !== null &&
      afterStray.held === 1 && afterStray.paused === 2 && afterStray.net === 2 &&
      afterStray.state.children === 0 && afterStray.state.errorAlert === false && afterStray.state.branchState !== 'error' &&
      JSON.stringify(afterStray.state) === JSON.stringify(preStray) &&
      afterStray.branchErrors === 0 &&
      finalB.children === 2 && finalB.hidden === false && finalB.expanded === 'true' &&
      counts.held === 0 && counts.paused === 2 && counts.net === 2 && pageErrors.length === 0,
    'doc1=' + JSON.stringify(doc1) + ' len=' + lenBefore + '->' + lenAfter + ' mid=' + JSON.stringify(mid) +
      ' stray=' + JSON.stringify(stray) + ' afterStray=' + JSON.stringify(afterStray) + ' finalB=' + JSON.stringify(finalB) +
      ' counts=' + JSON.stringify(counts) + ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * 3. 跨文档 pending 身份: 文档 1 的请求被整页交接遗弃; 文档 2 加载 P (held);
 *    在途再次激活 P 必须复用同一 held 请求 (无第三次请求); release 后单次
 *    安装 (scoped MutationObserver 计 cb-tree-node population/replacement),
 *    终态跟随最新意图 (收起), 第三次点击缓存命中展开零额外请求.
 * ===================================================================== */
await runScenario('3 cross-document pending identity: repeated activation reuses held request (no third request); single install', async () => {
  const { page, pageErrors } = await fresh('tree');
  const holdSet = new Set(['/adir']);
  const held = new Map();
  await holdTreePath(page, holdSet, held);
  let obsInstalled = false;
  try {
    // 文档 1: 发起请求后被整页交接遗弃
    await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
    await waitHeld(page, held, '/adir', 1, '3-doc1-held');
    await page.click('[data-cb-view="list"]');
    await waitFor(() => page.evaluate("!document.querySelector('ul#cb-tree-root')"), '3-doc2');
    await enterTree(page);
    // 文档 2: 进入链按持久化展开意图加载 adir -> held 请求 B (尚未被用户激活)
    await waitHeld(page, held, '/adir', 2, '3-doc2-held');
    const bId = (held.get('/adir') || [])[1].requestId;
    // scoped MutationObserver (文档 2 的 adir li): 只计 cb-tree-node 子项增删批次
    // = 实际 population/replacement 转换 (一次 install 恰一次 replaceChildren)
    const obsSetup = await page.evaluate(
      '(() => { const li = document.querySelector(' + JSON.stringify('[data-native-path-encoded="adir"]') + '); if (!li) return -1; window.__adirInstalls = 0; window.__adirObs = new MutationObserver(function (muts) { for (const m of muts) { for (const n of m.addedNodes) { if (n.nodeType === 1 && n.classList && n.classList.contains("cb-tree-node")) { window.__adirInstalls++; return; } } for (const n of m.removedNodes) { if (n.nodeType === 1 && n.classList && n.classList.contains("cb-tree-node")) { window.__adirInstalls++; return; } } } }); window.__adirObs.observe(li, { childList: true, subtree: true }); return 0; })()'
    );
    if (obsSetup !== 0) throw new Error('observer install failed: adir li missing in document 2');
    obsInstalled = true;
    const installs = () => page.evaluate('window.__adirInstalls');
    // 释放旧文档请求: 不得影响文档 2 的 pending 身份/DOM
    const stray = await releaseOne(page, held, '/adir', { tolerateError: true });
    await sleep(500);
    const afterStray = {
      held: (held.get('/adir') || []).length,
      sameB: (held.get('/adir') || [])[0]?.requestId === bId,
      paused: pausedCounts['/adir'] || 0,
      net: netCounts['/adir'] || 0,
      installs: await installs(),
    };
    // 在途再次激活: 翻转意图 (收起), 复用 B — 若身份被误删将发起第三次请求
    await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
    await sleep(300);
    const afterToggle = {
      held: (held.get('/adir') || []).length,
      sameB: (held.get('/adir') || [])[0]?.requestId === bId,
      paused: pausedCounts['/adir'] || 0,
      net: netCounts['/adir'] || 0,
      installs: await installs(),
    };
    // release B: 单次安装, 终态跟随最新意图 (收起, 子项已安装)
    await releaseOne(page, held, '/adir');
    await waitFor(
      () => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"adir\"] ul.cb-tree-children li.cb-tree-node')"),
      '3-landed'
    );
    await sleep(400);
    const final = await dirState(page, 'adir');
    const finalSel = await selState(page);
    const finalExpanded = await expandedList(page);
    const installsAfterRelease = await installs();
    // 第三次点击: 缓存命中展开, 零额外请求
    await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
    await sleep(300);
    const reopened = await dirState(page, 'adir');
    const counts = {
      held: (held.get('/adir') || []).length,
      paused: pausedCounts['/adir'] || 0,
      net: netCounts['/adir'] || 0,
    };
    rec('3 cross-document pending identity: repeated activation reuses held request (no third request); single install',
      stray !== null &&
        afterStray.held === 1 && afterStray.sameB === true && afterStray.paused === 2 && afterStray.net === 2 && afterStray.installs === 0 &&
        afterToggle.held === 1 && afterToggle.sameB === true && afterToggle.paused === 2 && afterToggle.net === 2 && afterToggle.installs === 0 &&
        final.children === 2 && final.hidden === true && final.expanded === 'false' && final.branchState === null &&
        finalSel.selected === 'adir' && finalSel.adLive === true && finalSel.selTrue === 1 &&
        !(JSON.stringify(finalExpanded) || '').includes('adir') &&
        installsAfterRelease === 1 &&
        reopened.hidden === false && reopened.expanded === 'true' && reopened.children === 2 &&
        counts.held === 0 && counts.paused === 2 && counts.net === 2 &&
        pageErrors.length === 0,
      'stray=' + JSON.stringify(stray) + ' afterStray=' + JSON.stringify(afterStray) + ' afterToggle=' + JSON.stringify(afterToggle) +
        ' final=' + JSON.stringify(final) + ' sel=' + JSON.stringify(finalSel) + ' expanded=' + JSON.stringify(finalExpanded) +
        ' installsAfterRelease=' + installsAfterRelease + ' reopened=' + JSON.stringify(reopened) + ' counts=' + JSON.stringify(counts) +
        ' errors=' + JSON.stringify(pageErrors));
  } finally {
    // 场景内 finally: 断开 scoped MutationObserver (幂等). 被 held 的响应由
    // runScenario 的 finally 统一释放 (page.close -> Fetch.disable).
    if (obsInstalled) {
      try {
        await page.evaluate('window.__adirObs && window.__adirObs.disconnect()');
      } catch {
        /* 页面已关闭 */
      }
    }
  }
});

/* =====================================================================
 * D. 已缓存折叠祖先 -> 面板下钻子项: 展开再折叠 (无在途, DOM 即权威, 缓存
 *    仍在); 面板迷你列表 /adir 使祖先缓存命中展开 (零新请求), 下钻
 *    /adir/child.md 后子项 current/选中.
 * ===================================================================== */
await runScenario('D cached collapsed ancestor -> pane navigate child: ancestor expands from cache, child current/selected', async () => {
  const { page, pageErrors } = await fresh('tree');
  const holdSet = new Set([]);
  const held = new Map();
  await holdTreePath(page, holdSet, held);
  // 展开 adir (加载落地), 再折叠 (缓存仍在)
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await waitFor(
    () => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"adir/child.md\"]')"),
    'adir-children'
  );
  await sleep(300);
  const counts1 = { paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await waitFor(
    () => page.evaluate("document.querySelector('[data-native-path-encoded=\"adir\"] > ul.cb-tree-children')?.hidden === true"),
    'adir-collapsed'
  );
  const collapsed = await dirState(page, 'adir');
  // 面板迷你列表: /adir 再 /adir/child.md (祖先 reveal 走缓存命中)
  await waitFor(
    () => page.evaluate("!!document.querySelector('.cb-pane .cb-dir-mini a[data-native-path-encoded=\"adir\"]')"),
    'pane-mini-ready'
  );
  await page.click('.cb-pane .cb-dir-mini a[data-native-path-encoded="adir"]');
  await waitFor(() => page.evaluate("location.pathname === '/adir'"), 'nav-adir');
  const afterDirNav = await dirState(page, 'adir');
  await waitFor(
    () => page.evaluate("!!document.querySelector('.cb-pane .cb-dir-mini a[data-native-path-encoded=\"adir/child.md\"]')"),
    'pane-child-mini-ready'
  );
  await page.click('.cb-pane .cb-dir-mini a[data-native-path-encoded="adir/child.md"]');
  await waitFor(() => page.evaluate("location.pathname === '/adir/child.md'"), 'nav-child');
  await waitFor(
    () => page.evaluate("(() => { const li = document.querySelector('[data-native-path-encoded=\"adir\"]'); return li && li.getAttribute('aria-expanded') === 'true' && li.querySelector(':scope > ul.cb-tree-children')?.hidden === false; })()"),
    'adir-reveal-expanded'
  );
  await sleep(300);
  const s = {
    adir: await dirState(page, 'adir'),
    current: await currentPaths(page),
    sel: await selState(page),
  };
  const counts2 = { paused: pausedCounts['/adir'] || 0, net: netCounts['/adir'] || 0 };
  rec('D cached collapsed ancestor -> pane navigate child: ancestor expands from cache, child current/selected',
    collapsed.expanded === 'false' && collapsed.hidden === true &&
      afterDirNav.expanded === 'true' && afterDirNav.hidden === false &&
      counts1.paused === 1 && counts1.net === 1 &&
      s.adir.expanded === 'true' && s.adir.hidden === false && s.adir.children === 2 &&
      JSON.stringify(s.current) === JSON.stringify(['adir', 'adir/child.md']) &&
      s.sel.selected === 'adir/child.md' && s.sel.adLive === true && s.sel.selTrue === 1 &&
      counts2.paused === 1 && counts2.net === 1 &&
      pageErrors.length === 0,
    'collapsed=' + JSON.stringify(collapsed) + ' afterDirNav=' + JSON.stringify(afterDirNav) +
      ' s=' + JSON.stringify(s) + ' counts1=' + JSON.stringify(counts1) + ' counts2=' + JSON.stringify(counts2) +
      ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * E. 重复持久化空目录: 进入链按展开意图 walk 两次 emptydir — 同版本共享
 *    结果, 恰一次请求; 空分支保留 expandable, branch-state=empty (不永久
 *    叶子化, 不删展开意图); 选项版本变化后重取一次.
 * ===================================================================== */
await runScenario('E duplicated persisted empty dir: one request, expandable kept, branch-state empty; options change refetches', async () => {
  const { page, pageErrors } = await fresh('list');
  await page.evaluate("localStorage.setItem('cb-tree-expanded', JSON.stringify(['emptydir', 'emptydir']))");
  await page.evaluate('window.__cbSeedMarker = 1');
  await holdTreePath(page, new Set(), new Map());
  await page.goto(BASE + '/'); // 新文档读取种子展开意图
  await waitFor(() => page.evaluate('typeof window.__cbSeedMarker === "undefined"'), 'seed-document', 12000);
  await enterTree(page);
  await waitFor(
    () => page.evaluate("document.querySelector('[data-native-path-encoded=\"emptydir\"]')?.getAttribute('data-cb-branch-state') === 'empty'"),
    'emptydir-empty'
  );
  await sleep(300);
  const first = await dirState(page, 'emptydir');
  const list1 = await expandedList(page);
  const counts1 = { paused: pausedCounts['/emptydir'] || 0, net: netCounts['/emptydir'] || 0 };
  // 同版本内折叠/再展开: 空分支不重复加载
  await page.click('[data-native-path-encoded="emptydir"] .cb-tree-row');
  await waitFor(
    () => page.evaluate("document.querySelector('[data-native-path-encoded=\"emptydir\"] > ul.cb-tree-children')?.hidden === true"),
    'emptydir-collapsed'
  );
  await page.click('[data-native-path-encoded="emptydir"] .cb-tree-row');
  await waitFor(
    () => page.evaluate("document.querySelector('[data-native-path-encoded=\"emptydir\"]')?.getAttribute('data-cb-branch-state') === 'empty'"),
    'emptydir-empty-again'
  );
  await sleep(300);
  const counts2 = { paused: pausedCounts['/emptydir'] || 0, net: netCounts['/emptydir'] || 0 };
  // 选项版本变化: 空分支失效并重取
  await page.click('[data-cb-hidden-toggle]');
  await waitFor(() => (pausedCounts['/emptydir'] || 0) > counts2.paused, 'emptydir-refetch', 8000);
  await sleep(400);
  const counts3 = { paused: pausedCounts['/emptydir'] || 0, net: netCounts['/emptydir'] || 0 };
  const list2 = await expandedList(page);
  const third = await dirState(page, 'emptydir');
  rec('E duplicated persisted empty dir: one request, expandable kept, branch-state empty; options change refetches',
    first.expandable === '1' && first.expanded === 'true' && first.children === 0 &&
      first.branchState === 'empty' && first.errorAlert === false &&
      counts1.paused === 1 && counts1.net === 1 &&
      counts2.paused === 1 && counts2.net === 1 &&
      Array.isArray(list1) && list1.includes('emptydir') &&
      Array.isArray(list2) && list2.includes('emptydir') &&
      counts3.paused === 2 && counts3.net === 2 &&
      third.expandable === '1' && third.branchState === 'empty' && third.children === 0 &&
      pageErrors.length === 0,
    'first=' + JSON.stringify(first) + ' list1=' + JSON.stringify(list1) + ' counts1=' + JSON.stringify(counts1) +
      ' counts2=' + JSON.stringify(counts2) + ' counts3=' + JSON.stringify(counts3) + ' list2=' + JSON.stringify(list2) +
      ' third=' + JSON.stringify(third) + ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * Q17T. 隐藏焦点回退 (树): 打开 a.md 内容; 显示隐藏后展开 mixdir 并选中/展开
 *    其中隐藏目录 .hiddendir; 隐藏该项 -> 回退到最近可见祖先 mixdir, 已打开
 *    内容与 pathname/q/hash 保留, 隐藏项不被打开; 再次显示 -> 分支恢复展开
 *    (重取) 且不抢焦点.
 * ===================================================================== */
await runScenario('Q17 tree: hiding selected hidden branch falls back to nearest visible ancestor, keeps opened doc; re-show restores expanded branch without focus steal', async () => {
  const { page, pageErrors } = await fresh('tree');
  const holdSet = new Set([]);
  const held = new Map();
  await holdTreePath(page, holdSet, held); // 只计数, 全部放行
  // 已打开内容: a.md 片段
  await page.click('[data-native-path-encoded="a.md"] .cb-tree-row');
  await waitFor(
    () => page.evaluate("!!document.querySelector('.cb-pane > .cb-fragment[data-native-path-encoded=\"a.md\"]')"),
    'a-doc-open'
  );
  await sleep(200);
  const pane0 = await paneState(page);
  const len = await historyLen(page);
  // 显示隐藏项: 同位置 replace (树壳内不整页重载)
  await page.click('[data-cb-hidden-toggle]');
  await waitFor(() => page.evaluate("location.search.includes('hidden=1')"), 'hidden-1');
  await sleep(400);
  const show = { rules: await urlRules(page), len: await historyLen(page), toggle: await page.evaluate("document.querySelector('[data-cb-hidden-toggle]').getAttribute('aria-pressed')") };
  // 展开 mixdir -> 选中并展开隐藏目录 .hiddendir
  await waitFor(() => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"mixdir\"]')"), 'mixdir-node');
  await page.click('[data-native-path-encoded="mixdir"] .cb-tree-row');
  await waitFor(() => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"mixdir/.hiddendir\"]')"), 'hiddendir-node');
  await page.click('[data-native-path-encoded="mixdir/.hiddendir"] .cb-tree-row');
  await waitFor(() => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"mixdir/.hiddendir/.deep.md\"]')"), 'deep-node');
  // 键盘选中隐藏分支 (行点击在分支安装后会清空选中): Q17 回退源必须是已选中节点
  const picked = await selectTreeNodeByKeyboard(page, 'mixdir/.hiddendir');
  await sleep(200);
  const before = { sel: await selState(page), pane: await paneState(page), expanded: await expandedList(page) };
  const mixBefore = pausedCounts['/mixdir'] || 0;
  // 隐藏: 选中节点消失 -> 焦点回退最近可见祖先 mixdir; 不改 pathname/q/hash
  await page.click('[data-cb-hidden-toggle]');
  await waitFor(() => page.evaluate("!document.querySelector('[data-native-path-encoded=\"mixdir/.hiddendir\"]')"), 'hiddendir-gone');
  await sleep(500);
  const afterHide = {
    rules: await urlRules(page),
    len: await historyLen(page),
    toggle: await page.evaluate("document.querySelector('[data-cb-hidden-toggle]').getAttribute('aria-pressed')"),
    sel: await selState(page),
    pane: await paneState(page),
    expanded: await expandedList(page),
  };
  const mixAfterHide = await dirState(page, 'mixdir');
  const mixHiddenCount = pausedCounts['/mixdir'] || 0;
  // 再次显示: 隐藏分支恢复展开 (重取), 不抢焦点
  await page.click('[data-cb-hidden-toggle]');
  await waitFor(
    () => page.evaluate("(() => { const li = document.querySelector('[data-native-path-encoded=\"mixdir/.hiddendir\"]'); return !!li && li.getAttribute('aria-expanded') === 'true' && !!li.querySelector(':scope > ul.cb-tree-children > li.cb-tree-node'); })()"),
    'hiddendir-restored-expanded',
    10000
  );
  await sleep(400);
  const afterShow = { sel: await selState(page), pane: await paneState(page), expanded: await expandedList(page) };
  const mixShownCount = pausedCounts['/mixdir'] || 0;
  const hiddenDirId = await page.evaluate("(document.querySelector('[data-native-path-encoded=\"mixdir/.hiddendir\"]') || {}).id || null");
  rec('Q17 tree: hiding selected hidden branch falls back to nearest visible ancestor, keeps opened doc; re-show restores expanded branch without focus steal',
    pane0.content === 'a.md' && pane0.surface === 'reading' &&
      show.rules.hidden === '1' && show.len === len && show.toggle === 'true' &&
      picked === true &&
      before.sel.selected === 'mixdir/.hiddendir' && before.pane.content === 'a.md' &&
      Array.isArray(before.expanded) && before.expanded.includes('mixdir/.hiddendir') &&
      afterHide.rules.pathname === '/a.md' && afterHide.rules.hidden === '0' && afterHide.rules.hash === '' && afterHide.len === len &&
      afterHide.toggle === 'false' &&
      afterHide.sel.selected === 'mixdir' && afterHide.sel.adLive === true &&
      afterHide.pane.content === 'a.md' && afterHide.pane.state !== 'error' &&
      Array.isArray(afterHide.expanded) && afterHide.expanded.includes('mixdir/.hiddendir') &&
      mixAfterHide.children === 1 && mixAfterHide.names.includes('visible.md') &&
      mixHiddenCount > mixBefore &&
      afterShow.sel.selected === 'mixdir' && afterShow.sel.ad !== hiddenDirId &&
      afterShow.pane.content === 'a.md' &&
      Array.isArray(afterShow.expanded) && afterShow.expanded.includes('mixdir/.hiddendir') &&
      mixShownCount > mixHiddenCount &&
      pageErrors.length === 0,
    'pane0=' + JSON.stringify(pane0) + ' show=' + JSON.stringify(show) + ' before=' + JSON.stringify(before) +
      ' afterHide=' + JSON.stringify(afterHide) + ' mixAfterHide=' + JSON.stringify(mixAfterHide) +
      ' counts=' + mixBefore + '/' + mixHiddenCount + '/' + mixShownCount + ' afterShow=' + JSON.stringify(afterShow) +
      ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * Q17L. 隐藏焦点回退 (列表): 显示隐藏 -> URL 原地替换为 hidden=1 (同位置
 *    replace, 历史长度/pathname/q/hash 不变); 选中隐藏行后隐藏 -> URL
 *    hidden=0, 隐藏行消失, 选中回退最近可见行, 隐藏行不被打开.
 * ===================================================================== */
await runScenario('Q17 list: toggle replaces hidden query in place; selection falls back to nearest visible row; hidden row not opened', async () => {
  const { page, pageErrors } = await fresh('list');
  const len = await historyLen(page);
  // 显示隐藏: URL gains hidden=1, 位置与历史不变
  await page.click('[data-cb-hidden-toggle]');
  await waitFor(
    () => page.evaluate("location.search.includes('hidden=1') && !!document.querySelector('#cb-view-list [data-native-path-encoded=\".hidden.md\"]')"),
    'hidden-row-visible'
  );
  await waitClientReady(page);
  await sleep(300);
  const shown = await listState(page);
  const shownLen = await historyLen(page);
  // 选中隐藏行
  await page.click('#cb-view-list [data-native-path-encoded=".hidden.md"]');
  await sleep(250);
  const selected = await listState(page);
  // 期望回退: 从隐藏行起先向后后向前找最近可见行 (与服务端排序后的行序一致)
  const expected = await page.evaluate(
    '(() => { const rows = Array.from(document.querySelectorAll("#cb-view-list [data-browser-entry]")); const idx = rows.findIndex((r) => r.dataset.nativePathEncoded === ".hidden.md"); if (idx < 0) return null; for (let d = 0; d < rows.length; d++) { for (const i of [idx + d, idx - d]) { if (i >= 0 && i < rows.length && i !== idx) { const p = rows[i].dataset.nativePathEncoded; const hidden = p.split("/").some((s) => s.indexOf(".") === 0); if (!hidden) return p; } } } return null; })()'
  );
  // 隐藏: URL hidden=0, 隐藏行消失, 选中回退
  await page.click('[data-cb-hidden-toggle]');
  await waitFor(
    () => page.evaluate("location.search.includes('hidden=0') && !document.querySelector('#cb-view-list [data-native-path-encoded=\".hidden.md\"]')"),
    'hidden-row-gone'
  );
  await waitClientReady(page);
  await sleep(450);
  const hidden = await listState(page);
  const hiddenLen = await historyLen(page);
  rec('Q17 list: toggle replaces hidden query in place; selection falls back to nearest visible row; hidden row not opened',
    shown.rows.includes('.hidden.md') && shown.hiddenPressed === 'true' &&
      shown.pathname === '/' && shown.search.includes('hidden=1') && !shown.search.includes('q=') && shown.hash === '' && shown.query === null &&
      shownLen === len &&
      JSON.stringify(selected.selected) === JSON.stringify(['.hidden.md']) &&
      expected !== null && expected !== '.hidden.md' &&
      !hidden.rows.includes('.hidden.md') &&
      JSON.stringify(hidden.selected) === JSON.stringify([expected]) &&
      hidden.pathname === '/' && hidden.search.includes('hidden=0') && !hidden.search.includes('q=') && hidden.hash === '' &&
      hidden.hiddenPressed === 'false' && hidden.treeMode === false && hidden.listHidden === false &&
      hiddenLen === len &&
      pageErrors.length === 0,
    'len=' + len + '/' + shownLen + '/' + hiddenLen + ' shown=' + JSON.stringify(shown) + ' selected=' + JSON.stringify(selected) +
      ' expected=' + JSON.stringify(expected) + ' hidden=' + JSON.stringify(hidden) + ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * OH. 仅含隐藏孩子的普通目录往返 + 结构性不可展开: hidden=0 展开 onlyhidden
 *    -> branch-state=empty 且 expandable 保持; 显示 -> 重取后子项可见;
 *    隐藏 -> 再次 empty; 再显示 -> 恢复展开且选中不被抢走. 环符号链接
 *    (服务端 expandable=false) 保持无 aria-expanded/无 twist, 点击与
 *    ArrowRight 零请求.
 * ===================================================================== */
await runScenario('only-hidden-children dir roundtrip: empty stays expandable and refetches per rules; cycle symlink stays non-expandable', async () => {
  const { page, pageErrors } = await fresh('tree');
  const holdSet = new Set([]);
  const held = new Map();
  await holdTreePath(page, holdSet, held, { fragments: true });
  // hidden=0: 展开 -> 当前版本无可见孩子: empty, 不叶子化
  await page.click('[data-native-path-encoded="onlyhidden"] .cb-tree-row');
  await waitFor(
    () => page.evaluate("document.querySelector('[data-native-path-encoded=\"onlyhidden\"]')?.getAttribute('data-cb-branch-state') === 'empty'"),
    'onlyhidden-empty'
  );
  await sleep(250);
  // 经键盘选中 onlyhidden — empty/children 版本重取必须保持独立选中 identity.
  const picked = await selectTreeNodeByKeyboard(page, 'onlyhidden');
  await sleep(150);
  const empty0 = await dirState(page, 'onlyhidden');
  const sel0 = await selState(page);
  const c0 = pausedCounts['/onlyhidden'] || 0;
  // 显示隐藏: 版本变化 -> 重取, 子项可见; 选中不被抢走
  // 先让目录内容区完成同规则刷新, 再释放慢分支, 不依赖网络偶然先后.
  holdSet.add('/onlyhidden');
  holdSet.add('fragment:/');
  await page.click('[data-cb-hidden-toggle]');
  await waitFor(() => (pausedCounts['/onlyhidden'] || 0) > c0, 'onlyhidden-refetch-show', 8000);
  await waitHeld(page, held, '/onlyhidden', 1, 'onlyhidden-show-held');
  await waitHeld(page, held, 'fragment:/', 1, 'directory-pane-show-held');
  holdSet.delete('fragment:/');
  await releaseOne(page, held, 'fragment:/');
  await waitFor(
    () => page.evaluate("!!document.querySelector('.cb-pane .cb-dir-fragment[data-cb-hidden=\"1\"]')"),
    'directory-pane-show-committed'
  );
  holdSet.delete('/onlyhidden');
  await releaseOne(page, held, '/onlyhidden');
  await waitFor(
    () => page.evaluate("(() => { const li = document.querySelector('[data-native-path-encoded=\"onlyhidden\"]'); return !!li && li.getAttribute('data-cb-branch-state') === null && !!li.querySelector(':scope > ul.cb-tree-children > li.cb-tree-node bdi'); })()"),
    'onlyhidden-children'
  );
  await sleep(250);
  const shown = await dirState(page, 'onlyhidden');
  const selShown = await selState(page);
  // 隐藏: 再次 empty (重取)
  const c1 = pausedCounts['/onlyhidden'] || 0;
  await page.click('[data-cb-hidden-toggle]');
  await waitFor(() => (pausedCounts['/onlyhidden'] || 0) > c1, 'onlyhidden-refetch-hide', 8000);
  await waitFor(
    () => page.evaluate("document.querySelector('[data-native-path-encoded=\"onlyhidden\"]')?.getAttribute('data-cb-branch-state') === 'empty'"),
    'onlyhidden-hidden-empty'
  );
  const empty1 = await dirState(page, 'onlyhidden');
  const selHidden = await selState(page);
  // 再显示: 恢复展开 (重取) 且不抢焦点
  const c2 = pausedCounts['/onlyhidden'] || 0;
  await page.click('[data-cb-hidden-toggle]');
  await waitFor(() => (pausedCounts['/onlyhidden'] || 0) > c2, 'onlyhidden-refetch-reshow', 8000);
  await waitFor(
    () => page.evaluate("(() => { const li = document.querySelector('[data-native-path-encoded=\"onlyhidden\"]'); return !!li && !!li.querySelector(':scope > ul.cb-tree-children > li.cb-tree-node'); })()"),
    'onlyhidden-reshown'
  );
  await sleep(300);
  const shown2 = await dirState(page, 'onlyhidden');
  const selShown2 = await selState(page);
  // 结构性不可展开: adir/grand/loop (服务端 expandable=false)
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await waitFor(() => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"adir/grand\"]')"), 'grand-node');
  await page.click('[data-native-path-encoded="adir/grand"] .cb-tree-row');
  await waitFor(() => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"adir/grand/loop\"]')"), 'loop-node');
  await sleep(200);
  const loopBefore = await dirState(page, 'adir/grand/loop');
  const loopCounts0 = { paused: pausedCounts['/adir/grand/loop'] || 0, net: netCounts['/adir/grand/loop'] || 0 };
  await page.click('[data-native-path-encoded="adir/grand/loop"] .cb-tree-row');
  await page.press('ArrowRight', 'ArrowRight', 39);
  await sleep(400);
  const loopAfter = await dirState(page, 'adir/grand/loop');
  const loopCounts1 = { paused: pausedCounts['/adir/grand/loop'] || 0, net: netCounts['/adir/grand/loop'] || 0 };
  rec('only-hidden-children dir roundtrip: empty stays expandable and refetches per rules; cycle symlink stays non-expandable',
    empty0.expandable === '1' && empty0.branchState === 'empty' && empty0.expanded === 'true' && empty0.children === 0 &&
      empty0.errorAlert === false && picked === true && sel0.selected === 'onlyhidden' &&
      shown.expandable === '1' && shown.branchState === null && shown.children === 1 && shown.names.includes('.inside.md') &&
      selShown.selected === 'onlyhidden' && selShown.adLive === true &&
      empty1.expandable === '1' && empty1.branchState === 'empty' && empty1.children === 0 &&
      selHidden.selected === 'onlyhidden' &&
      shown2.expandable === '1' && shown2.branchState === null && shown2.children === 1 &&
      selShown2.selected === 'onlyhidden' && selShown2.adLive === true &&
      loopBefore.expandable === '0' && loopBefore.expanded === null && loopBefore.hasUl === false && loopBefore.branchState === null &&
      loopAfter.expandable === '0' && loopAfter.hasUl === false && loopAfter.branchState === null &&
      loopCounts0.paused === 0 && loopCounts0.net === 0 && loopCounts1.paused === 0 && loopCounts1.net === 0 &&
      pageErrors.length === 0,
    'empty0=' + JSON.stringify(empty0) + ' sel0=' + JSON.stringify(sel0) + ' shown=' + JSON.stringify(shown) +
      ' selShown=' + JSON.stringify(selShown) + ' empty1=' + JSON.stringify(empty1) + ' selHidden=' + JSON.stringify(selHidden) +
      ' shown2=' + JSON.stringify(shown2) + ' selShown2=' + JSON.stringify(selShown2) +
      ' loopBefore=' + JSON.stringify(loopBefore) + ' loopAfter=' + JSON.stringify(loopAfter) +
      ' loopCounts=' + JSON.stringify(loopCounts0) + '/' + JSON.stringify(loopCounts1) +
      ' counts=' + [c0, c1, c2].join('/') + ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * V. 两个已展开分支部分失败 + 旧版本响应: hidden=0 -> 1 时 adir 注入 500,
 *    bdir 成功 — 成功分支装新数据 (隐藏孩子出现), 失败分支显示安全错误
 *    (branch-state=error + role=alert, 不留旧集合, 不视为空目录), 已打开
 *    文档与展开意图保留; 排序再变 (V2 在途), 又变 (V3) 后释放 V2 响应:
 *    零安装/零错误 UI/零陈旧变异/无自动重试; release V3 成功安装.
 * ===================================================================== */
await runScenario('two expanded branches refresh: one installs new data, one current-version error; released stale response commits nothing, no auto retry', async () => {
  const { page, pageErrors } = await fresh('tree', { patchAbort: true });
  // 先在无拦截下展开分支并打开文档 (V0)
  await page.click('[data-native-path-encoded="a.md"] .cb-tree-row');
  await waitFor(
    () => page.evaluate("!!document.querySelector('.cb-pane > .cb-fragment[data-native-path-encoded=\"a.md\"]')"),
    'a-open'
  );
  await page.click('[data-native-path-encoded="adir"] .cb-tree-row');
  await waitFor(
    () => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"adir\"] ul.cb-tree-children > li.cb-tree-node')"),
    'adir-v0'
  );
  await page.click('[data-native-path-encoded="bdir"] .cb-tree-row');
  await waitFor(
    () => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"bdir\"] ul.cb-tree-children > li.cb-tree-node')"),
    'bdir-v0'
  );
  await sleep(200);
  const v0 = { adir: await dirState(page, 'adir'), bdir: await dirState(page, 'bdir'), pane: await paneState(page), len: await historyLen(page) };
  // 从这里开始拦截: V1 (hidden=1) 下 adir 500, bdir held
  const holdSet = new Set(['/adir', '/bdir']);
  const held = new Map();
  await holdTreePath(page, holdSet, held, { failOnce: new Map([['/adir', 1]]) });
  await page.click('[data-cb-hidden-toggle]');
  await waitHeld(page, held, '/bdir', 1, 'V1-bdir-held');
  await waitFor(
    () => page.evaluate("document.querySelector('[data-native-path-encoded=\"adir\"]')?.getAttribute('data-cb-branch-state') === 'error'"),
    'v1-adir-error'
  );
  await sleep(300);
  const v1fail = { adir: await dirState(page, 'adir'), pane: await paneState(page), expanded: await expandedList(page) };
  await releaseOne(page, held, '/bdir');
  await waitFor(
    () => page.evaluate("!!document.querySelector('[data-native-path-encoded=\"bdir\"] ul.cb-tree-children > li.cb-tree-node')"),
    'v1-bdir-landed'
  );
  await sleep(300);
  const v1ok = await dirState(page, 'bdir');
  // V2: 排序变化 -> 新版本分支请求全部暂停 (未释放)
  await page.click('[data-cb-sort="Size:Desc"]');
  await waitHeld(page, held, '/adir', 1, 'V2-adir-held');
  await waitHeld(page, held, '/bdir', 1, 'V2-bdir-held');
  await sleep(200);
  // V3: 再变排序 (V2 意图被中止, abort 伪装 no-op -> 旧请求仍可释放; V3 请求暂停)
  await page.click('[data-cb-sort="Name:Desc"]');
  await waitHeld(page, held, '/adir', 2, 'V3-adir-held');
  await waitHeld(page, held, '/bdir', 2, 'V3-bdir-held');
  await sleep(300);
  const staleBefore = {
    adir: await dirState(page, 'adir'),
    bdir: await dirState(page, 'bdir'),
    branchErrors: await page.evaluate("document.querySelectorAll('.cb-tree-branch-error').length"),
    rootChildren: await page.evaluate("document.querySelectorAll('#cb-tree-root > li.cb-tree-node').length"),
  };
  const staleCounts0 = {
    adir: pausedCounts['/adir'] || 0,
    bdir: pausedCounts['/bdir'] || 0,
    netAdir: netCounts['/adir'] || 0,
    netBdir: netCounts['/bdir'] || 0,
  };
  // 释放旧版本 (V2: sort=Size:Desc) 的 adir 响应: 零提交/零错误/零变异/无自动重试
  const stale = await releaseOne(page, held, '/adir', { sort: 'Size:Desc', hidden: '1' });
  const staleError = await releaseOne(page, held, '/bdir', { sort: 'Size:Desc', hidden: '1', fail: true });
  await sleep(800);
  const staleAfter = {
    adir: await dirState(page, 'adir'),
    bdir: await dirState(page, 'bdir'),
    branchErrors: await page.evaluate("document.querySelectorAll('.cb-tree-branch-error').length"),
    rootChildren: await page.evaluate("document.querySelectorAll('#cb-tree-root > li.cb-tree-node').length"),
  };
  const staleCounts1 = {
    adir: pausedCounts['/adir'] || 0,
    bdir: pausedCounts['/bdir'] || 0,
    netAdir: netCounts['/adir'] || 0,
    netBdir: netCounts['/bdir'] || 0,
  };
  // 旧响应/错误结束后再次激活当前在途分支: 仍复用原请求, 不发第三次 fetch.
  const pendingIds = ['/adir', '/bdir'].map((path) => held.get(path)?.[0]?.requestId);
  for (const path of ['adir', 'bdir', 'adir', 'bdir']) {
    await page.click(`[data-native-path-encoded="${path}"] .cb-tree-row`);
  }
  await sleep(300);
  const pendingKept = pendingIds.every(
    (id, i) => id && held.get(['/adir', '/bdir'][i])?.[0]?.requestId === id
  );
  const activationCounts = { adir: netCounts['/adir'] || 0, bdir: netCounts['/bdir'] || 0 };
  // release V3 (当前版本): 成功安装
  const v3 = await releaseOne(page, held, '/adir', { sort: 'Name:Desc', hidden: '1' });
  const v3bdir = await releaseOne(page, held, '/bdir', { sort: 'Name:Desc', hidden: '1' });
  await waitFor(
    () => page.evaluate("(() => { const li = document.querySelector('[data-native-path-encoded=\"adir\"]'); return li && li.getAttribute('data-cb-branch-state') === null && !!li.querySelector(':scope > ul.cb-tree-children > li.cb-tree-node'); })()"),
    'v3-adir-landed'
  );
  await waitFor(
    () => page.evaluate("document.querySelector('[data-native-path-encoded=\"bdir\"]')?.getAttribute('data-cb-branch-state') === null && document.querySelectorAll('[data-native-path-encoded=\"bdir\"] ul.cb-tree-children > li.cb-tree-node').length === 2"),
    'v3-bdir-landed'
  );
  await sleep(300);
  const finalAdir = await dirState(page, 'adir');
  const finalBdir = await dirState(page, 'bdir');
  const finalPane = await paneState(page);
  const finalLen = await historyLen(page);
  rec('two expanded branches refresh: one installs new data, one current-version error; released stale response commits nothing, no auto retry',
    v0.adir.children === 2 && v0.bdir.children === 1 && v0.bdir.names.includes('x.md') && !v0.bdir.names.includes('.bh.md') &&
      v0.pane.content === 'a.md' &&
      v1fail.adir.branchState === 'error' && v1fail.adir.errorAlert === true &&
      v1fail.adir.children === 0 && v1fail.adir.expanded === 'true' &&
      Array.isArray(v1fail.expanded) && v1fail.expanded.includes('adir') && v1fail.expanded.includes('bdir') &&
      v1fail.pane.content === 'a.md' && v1fail.pane.state !== 'error' &&
      v1ok.branchState === null && v1ok.children === 2 && v1ok.names.includes('.bh.md') && v1ok.names.includes('x.md') &&
      stale !== null && stale.sort === 'Size:Desc' &&
      staleError !== null && staleError.sort === 'Size:Desc' &&
      JSON.stringify(staleAfter) === JSON.stringify(staleBefore) &&
      staleCounts1.adir === staleCounts0.adir && staleCounts1.bdir === staleCounts0.bdir &&
      staleCounts1.netAdir === staleCounts0.netAdir && staleCounts1.netBdir === staleCounts0.netBdir &&
      v3 !== null && finalAdir.branchState === null && finalAdir.children === 2 && finalAdir.hidden === false && finalAdir.expanded === 'true' &&
      pendingKept && activationCounts.adir === staleCounts1.netAdir && activationCounts.bdir === staleCounts1.netBdir &&
      v3bdir !== null && finalBdir.branchState === null && finalBdir.children === 2 && finalBdir.expanded === 'true' &&
      finalPane.content === 'a.md' && finalLen === v0.len &&
      pageErrors.length === 0,
    'v0=' + JSON.stringify(v0) + ' v1fail=' + JSON.stringify(v1fail) + ' v1ok=' + JSON.stringify(v1ok) +
      ' stale=' + JSON.stringify(stale) + ' staleBefore=' + JSON.stringify(staleBefore) + ' staleAfter=' + JSON.stringify(staleAfter) +
      ' staleError=' + JSON.stringify(staleError) + ' pendingKept=' + pendingKept + ' activationCounts=' + JSON.stringify(activationCounts) +
      ' staleCounts=' + JSON.stringify(staleCounts0) + '/' + JSON.stringify(staleCounts1) +
      ' finalAdir=' + JSON.stringify(finalAdir) + ' finalPane=' + JSON.stringify(finalPane) + ' len=' + v0.len + '/' + finalLen +
      ' finalBdir=' + JSON.stringify(finalBdir) +
      ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * H. 历史恢复失败 + 恢复: 面板 /adir 成功安装 -> 打开 /a.md; 对下一次 /adir
 *    fragment 注入 500 后 popstate 恢复 /adir — 目标 URL/快照保留, 面板
 *    .cb-pane[data-cb-pane-state=error] + .cb-pane-error[role=alert], 成功
 *    安装标记失效; 同路径再次恢复必须重取 (计数递增) 并成功安装.
 * ===================================================================== */
await runScenario('history restore failure: target kept, pane error state, success marker invalidated; same-path restore refetches and installs', async () => {
  const { page, pageErrors } = await fresh('tree');
  // 面板导航: /adir 成功安装 (fragment #1)
  await waitFor(
    () => page.evaluate("!!document.querySelector('.cb-pane .cb-dir-mini a[data-native-path-encoded=\"adir\"]')"),
    'pane-mini-ready'
  );
  await page.click('.cb-pane .cb-dir-mini a[data-native-path-encoded="adir"]');
  await waitFor(
    () => page.evaluate("location.pathname === '/adir' && !!document.querySelector('.cb-pane > .cb-doc[data-native-path-encoded=\"adir\"]')"),
    'adir-committed'
  );
  const c1 = fragCounts['/adir'] || 0;
  // 打开文档 -> /a.md (fragment)
  await page.click('[data-native-path-encoded="a.md"] .cb-tree-row');
  await waitFor(
    () => page.evaluate("location.pathname === '/a.md' && !!document.querySelector('.cb-pane > .cb-fragment[data-native-path-encoded=\"a.md\"]')"),
    'a-committed'
  );
  // 下一次 /adir fragment 注入 500, 然后 popstate 恢复 /adir
  const fail = { path: '/adir', remaining: 1, failed: 0 };
  await failNextFragment(page, fail);
  await page.evaluate('history.back()');
  await waitFor(
    () => page.evaluate("location.pathname === '/adir' && document.querySelector('.cb-pane')?.getAttribute('data-cb-pane-state') === 'error'"),
    'adir-error'
  );
  await sleep(300);
  const c2 = fragCounts['/adir'] || 0;
  const errState = await paneState(page);
  const snapAtError = await page.evaluate(
    "JSON.stringify(history.state && history.state.cb ? { path: history.state.cb.path, surface: history.state.cb.surface } : null)"
  );
  // 同路径再次恢复: 成功标记已失效 -> 必须重取, 且这次安装内容
  await page.evaluate('history.forward()');
  await waitFor(
    () => page.evaluate("location.pathname === '/a.md' && !!document.querySelector('.cb-pane > .cb-fragment[data-native-path-encoded=\"a.md\"]')"),
    'a-again'
  );
  await page.evaluate('history.back()');
  await waitFor(
    () => page.evaluate("location.pathname === '/adir' && !!document.querySelector('.cb-pane > .cb-doc[data-native-path-encoded=\"adir\"]')"),
    'adir-retry-landed'
  );
  await sleep(300);
  const c3 = fragCounts['/adir'] || 0;
  const retryState = await paneState(page);
  rec('history restore failure: target kept, pane error state, success marker invalidated; same-path restore refetches and installs',
    c1 === 1 && fail.failed === 1 && c2 === 2 &&
      errState.state === 'error' && errState.error === true && errState.errorText.length > 0 &&
      snapAtError !== null && JSON.parse(snapAtError).path === 'adir' && JSON.parse(snapAtError).surface === 'directory' &&
      c3 === 3 && retryState.state === 'ready' && retryState.error === false &&
      retryState.content === 'adir' && retryState.miniLinks === 2 &&
      pageErrors.length === 0,
    'fragCounts=' + c1 + '/' + c2 + '/' + c3 + ' fail=' + JSON.stringify(fail) + ' errState=' + JSON.stringify(errState) +
      ' snap=' + snapAtError + ' retryState=' + JSON.stringify(retryState) + ' errors=' + JSON.stringify(pageErrors));
});

/* =====================================================================
 * L. reload + 跨整页后退/前进 + BFCache 观察: 另一个 tab 改偏好后, 真实
 *    reload 必须按 history.state.cb 恢复视图/规则/已打开条目且不写偏好;
 *    tree -> list 整页交接 (同路径 replace, 历史长度不变); 跨整页后退/前进
 *    恢复 surface/layout (list vs tree) 与搜索 q, 仍不写偏好; pageshow 记录
 *    聚合到 sessionStorage — 本进程 (保留 --disable-features=BackForwardCache)
 *    预期非 persisted, 此时断言快照恢复并记录 BFCache 限制; 若观察到
 *    persisted 则断言恢复零 /__/tree 与 fragment 重取.
 * ===================================================================== */
await runScenario('reload and cross-full-page back/forward restore snapshot (view/rules/opened/q) without writing prefs; persisted pageshow observed only if BFCache available', async () => {
  const { page, pageErrors } = await fresh('list');
  // 每个新文档安装 pageshow 记录器, 聚合到 sessionStorage (跨整页导航/后退)
  await page.send('Page.addScriptToEvaluateOnNewDocument', {
    source:
      "(() => { const push = (persisted) => { let list = []; try { list = JSON.parse(sessionStorage.getItem('__cbLifecycle') || '[]'); } catch (e) { list = []; } list.push({ persisted: !!persisted, href: location.pathname + location.search, at: Date.now() }); try { sessionStorage.setItem('__cbLifecycle', JSON.stringify(list.slice(-40))); } catch (e) {} }; window.addEventListener('pageshow', (e) => push(e.persisted)); })()",
  });
  await page.evaluate('window.__cbAdirMarker = 1');
  await page.goto(BASE + '/adir?sort=Size:Desc&hidden=1');
  await waitFor(
    () => page.evaluate("typeof window.__cbAdirMarker === \"undefined\" && location.pathname === '/adir'"),
    'adir-document',
    12000
  );
  await waitFor(() => page.evaluate("!!document.querySelector('#cb-view-list [data-native-path-encoded=\"adir/child.md\"]')"), 'adir-list');
  // 打开当前目录内的 child.md 预览 = 已打开条目 (snapshot.opened)
  await page.click('#cb-view-list [data-native-path-encoded="adir/child.md"] [data-cb-action="preview"]');
  await waitFor(
    () => page.evaluate("(() => { const p = document.querySelector('#cb-preview'); return !!p && !p.hidden && !!p.querySelector('[data-native-path-encoded=\"adir/child.md\"]'); })()"),
    'preview-open'
  );
  const snap0 = await page.evaluate(
    'JSON.stringify(history.state && history.state.cb ? { surface: history.state.cb.surface, view: history.state.cb.view, path: history.state.cb.path, sort: history.state.cb.sort, hidden: history.state.cb.hidden, opened: history.state.cb.opened } : null)'
  );
  const len0 = await historyLen(page);
  // 另一个 tab 改偏好; reload 必须按快照恢复而不是偏好, 且不写偏好
  const other1 = await setOtherTabPrefs({ 'cb-browse-view': 'tree', 'cb-sort': 'Name:Asc', 'cb-hidden': '0' });
  await page.evaluate('window.__cbReloadMarker = 1');
  await page.send('Page.reload');
  // 新文档判据: 旧文档的 window 标记不存在; 避免把 reload 前的旧文档状态当真
  await waitFor(() => page.evaluate('typeof window.__cbReloadMarker === "undefined"'), 'reload-document', 12000);
  await page.waitLoad();
  await waitFor(
    () => page.evaluate("!document.body.classList.contains('cb-tree-mode') && !!document.querySelector('#cb-view-list')"),
    'reload-list'
  );
  await waitClientReady(page);
  await waitFor(
    () => page.evaluate("(() => { const p = document.querySelector('#cb-preview'); return !!p && !p.hidden && !!p.querySelector('[data-native-path-encoded=\"adir/child.md\"]'); })()"),
    'reload-preview'
  );
  await sleep(300);
  const afterReload = { state: await listState(page), rules: await urlRules(page), prefs: await prefsState(page), len: await historyLen(page) };
  const lifeReload = await page.evaluate("JSON.parse(sessionStorage.getItem('__cbLifecycle') || '[]')");
  // 独立打开的预览已由 reload 验证; 关闭后专门测当前目录的同位置布局交接.
  await page.click('#cb-preview [data-cb-action="close"]');
  await waitFor(() => page.evaluate("document.querySelector('#cb-preview')?.hidden === true"), 'preview-closed');
  // 树 -> 列表整页交接: 同路径 replace + reload, 历史长度不变
  await page.click('[data-cb-view="tree"]');
  await waitFor(() => page.evaluate("document.body.classList.contains('cb-tree-mode')"), 'tree-again');
  const lenBeforeHandoff = await historyLen(page);
  await page.evaluate('window.__cbHandoffMarker = 1');
  await page.click('[data-cb-view="list"]');
  await waitFor(
    () => page.evaluate("typeof window.__cbHandoffMarker === \"undefined\" && location.pathname === '/adir' && !!document.querySelector('#cb-view-list')"),
    'handoff-list',
    12000
  );
  const lenAfterHandoff = await historyLen(page);
  await waitClientReady(page);
  // 打开文档整页 (preview -> full 当前 tab)
  await page.click('#cb-view-list [data-native-path-encoded="adir/child.md"] [data-cb-action="preview"]');
  await waitFor(
    () => page.evaluate("(() => { const p = document.querySelector('#cb-preview'); return !!p && !p.hidden && !!p.querySelector('[data-native-path-encoded=\"adir/child.md\"]') && p.querySelector('[data-cb-action=\"full\"]').disabled === false; })()"),
    'preview2'
  );
  await page.click('#cb-preview [data-cb-action="full"]');
  await waitFor(
    () => page.evaluate("location.pathname === '/adir/child.md' && document.body.classList.contains('cb-doc-page')"),
    'full-a'
  );
  // 另一个 tab 再改偏好 -> 跨整页后退/前进按快照恢复, 不回写偏好
  const other2 = await setOtherTabPrefs({ 'cb-browse-view': 'grid', 'cb-sort': 'Name:Desc', 'cb-hidden': '0' });
  const flight0 = { tree: netCounts['/'] || 0, frag: fragCounts['/adir'] || 0 };
  await page.evaluate('history.back()');
  await waitFor(
    () => page.evaluate("location.pathname === '/adir' && !document.body.classList.contains('cb-tree-mode') && !!document.querySelector('#cb-view-list')"),
    'back-adir'
  );
  await waitClientReady(page);
  await sleep(400);
  const back = { state: await listState(page), rules: await urlRules(page), prefs: await prefsState(page) };
  const flight1 = { tree: netCounts['/'] || 0, frag: fragCounts['/adir'] || 0 };
  await page.evaluate('history.forward()');
  await waitFor(
    () => page.evaluate("location.pathname === '/adir/child.md' && document.body.classList.contains('cb-doc-page')"),
    'forward-a'
  );
  await waitClientReady(page);
  await sleep(300);
  const fwd = await page.evaluate(
    '(() => { const nav = document.querySelector(".cb-reading-nav"); const parent = document.querySelector("[data-cb-reading-parent]"); return { reading: !!nav && nav.getClientRects().length > 0 && !document.body.classList.contains("cb-tree-mode"), parent: parent ? new URL(parent.href).pathname : null }; })()'
  );
  // 搜索: 跨视图保留 q/sort/hidden; tree -> list 同位置 replace + reload 不增历史
  await page.click('.cb-reading-desktop .cb-reading-search > summary');
  await page.evaluate(
    '(() => { const input = document.querySelector(".cb-reading-desktop .cb-reading-search input[name=q]"); input.value = "leaf"; input.form.requestSubmit(); return true; })()'
  );
  await waitFor(
    () => page.evaluate("location.pathname === '/__/search' && document.body.dataset.cbQuery === 'leaf' && !!document.querySelector('#cb-view-list')"),
    'search-page'
  );
  await waitClientReady(page);
  const lenSearch0 = await historyLen(page);
  await page.click('[data-cb-view="tree"]');
  await waitFor(
    () => page.evaluate("document.body.classList.contains('cb-tree-mode') && !!document.querySelector('.cb-pane .cb-search-fragment')"),
    'search-tree'
  );
  const searchTree = await urlRules(page);
  await page.evaluate('window.__cbSearchHandoffMarker = 1');
  await page.click('[data-cb-view="list"]');
  await waitFor(
    () => page.evaluate("typeof window.__cbSearchHandoffMarker === \"undefined\" && document.body.dataset.cbQuery === 'leaf' && !!document.querySelector('#cb-view-list')"),
    'search-list',
    12000
  );
  await waitClientReady(page);
  const searchList = await urlRules(page);
  const lenSearch1 = await historyLen(page);
  // 另一个 tab 改偏好 -> 后退/前进仍恢复 q + 布局 + 规则, 且不写偏好
  const other3 = await setOtherTabPrefs({ 'cb-browse-view': 'grid', 'cb-sort': 'Type:Desc', 'cb-hidden': '1' });
  await page.evaluate('history.back()');
  await waitFor(() => page.evaluate("location.pathname === '/adir/child.md' && document.body.classList.contains('cb-doc-page')"), 'back-child');
  await waitClientReady(page);
  await sleep(300);
  const backA = await page.evaluate(
    '(() => { const nav = document.querySelector(".cb-reading-nav"); return { reading: !!nav && nav.getClientRects().length > 0 && !document.body.classList.contains("cb-tree-mode") }; })()'
  );
  await page.evaluate('history.forward()');
  await waitFor(
    () => page.evaluate("location.pathname === '/__/search' && document.body.dataset.cbQuery === 'leaf' && !!document.querySelector('#cb-view-list')"),
    'forward-search'
  );
  await waitClientReady(page);
  await sleep(300);
  const fwdSearch = { state: await listState(page), rules: await urlRules(page), prefs: await prefsState(page) };
  // 生命周期: BFCache 可用则恢复必须零 /__/tree 与 fragment 重取
  const life = await page.evaluate("JSON.parse(sessionStorage.getItem('__cbLifecycle') || '[]')");
  const persistedCount = life.filter((e) => e.persisted === true).length;
  let bfcacheNote;
  let zeroRefetch = true;
  if (persistedCount > 0) {
    zeroRefetch = flight1.tree === flight0.tree && flight1.frag === flight0.frag;
    bfcacheNote = 'persisted pageshow observed (' + persistedCount + '); no tree/fragment refetch across restore';
  } else {
    bfcacheNote = 'BFCache unavailable in this process (script keeps --disable-features=BackForwardCache); non-persisted pageshow restore verified instead';
  }
  rec('reload and cross-full-page back/forward restore snapshot (view/rules/opened/q) without writing prefs; persisted pageshow observed only if BFCache available',
    JSON.parse(snap0) !== null && JSON.parse(snap0).surface === 'directory' && JSON.parse(snap0).view === 'list' &&
      JSON.parse(snap0).path === 'adir' && JSON.parse(snap0).sort === 'Size:Desc' && JSON.parse(snap0).hidden === 1 &&
      JSON.parse(snap0).opened === 'adir/child.md' &&
      afterReload.len === len0 && afterReload.state.treeMode === false && afterReload.state.pressed.list === 'true' && afterReload.state.pressed.tree === 'false' &&
      afterReload.state.hiddenPressed === 'true' && afterReload.rules.sort === 'Size:Desc' && afterReload.rules.hidden === '1' &&
      afterReload.prefs.view === other1.view && afterReload.prefs.sort === other1.sort && afterReload.prefs.hidden === other1.hidden &&
      lifeReload.some((e) => e.persisted === false) &&
      lenAfterHandoff === lenBeforeHandoff &&
      back.state.pressed.list === 'true' && back.state.pressed.tree === 'false' && back.state.treeMode === false &&
      back.rules.pathname === '/adir' && back.rules.sort === 'Size:Desc' && back.rules.hidden === '1' &&
      back.prefs.view === other2.view && back.prefs.sort === other2.sort && back.prefs.hidden === other2.hidden &&
      fwd.reading && fwd.parent === '/adir' &&
      searchTree.pathname === '/__/search' && searchTree.q === 'leaf' && searchList.pathname === '/__/search' && searchList.q === 'leaf' &&
      searchList.sort === searchTree.sort && searchList.hidden === searchTree.hidden &&
      lenSearch1 === lenSearch0 &&
      backA.reading &&
      fwdSearch.state.pressed.list === 'true' && fwdSearch.state.pressed.grid === 'false' && fwdSearch.state.treeMode === false &&
      fwdSearch.state.query === 'leaf' && fwdSearch.rules.pathname === '/__/search' && fwdSearch.rules.q === 'leaf' &&
      fwdSearch.rules.sort === searchList.sort && fwdSearch.rules.hidden === searchList.hidden &&
      fwdSearch.prefs.view === other3.view && fwdSearch.prefs.sort === other3.sort && fwdSearch.prefs.hidden === other3.hidden &&
      zeroRefetch === true &&
      pageErrors.length === 0,
    'snap0=' + snap0 + ' afterReload=' + JSON.stringify(afterReload) + ' lifeReload=' + JSON.stringify(lifeReload.slice(-3)) +
      ' other1=' + JSON.stringify(other1) + ' lenHandoff=' + lenBeforeHandoff + '->' + lenAfterHandoff +
      ' back=' + JSON.stringify(back) + ' other2=' + JSON.stringify(other2) + ' fwd=' + JSON.stringify(fwd) +
      ' searchTree=' + JSON.stringify(searchTree) + ' searchList=' + JSON.stringify(searchList) + ' lenSearch=' + lenSearch0 + '->' + lenSearch1 +
      ' backA=' + JSON.stringify(backA) + ' fwdSearch=' + JSON.stringify(fwdSearch) + ' other3=' + JSON.stringify(other3) +
      ' persistedCount=' + persistedCount + ' bfcache=' + bfcacheNote + ' flight=' + JSON.stringify(flight0) + '->' + JSON.stringify(flight1) +
      ' errors=' + JSON.stringify(pageErrors));
});

/* ---------- 收尾: 汇总 + 清理 ---------- */

try {
  const pass = results.filter((r) => r.pass).length;
  console.log('SUMMARY: ' + pass + '/' + results.length);
  if (scenarioFilter && results.length === 0) throw new Error('No matching scenario: ' + scenarioFilter);
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
