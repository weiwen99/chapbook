/* chapbook-browser.js — 目录/搜索/文档页渐进增强 (vanilla IIFE, 无依赖/构建).
 *
 * 安全契约 (docs/2026-08-12-design-readonly-file-browser.org, browser action path
 * contract): `element.dataset.nativePathEncoded` 是行/面板唯一的 action target.
 * - URL actions (当前 tab / 新 tab / 预览 / 目录下钻 / 下载) 全程不 decode
 *   encodedPath: 统一经 actionUrl() 构造 URL, 模式只经 URL.searchParams 添加;
 * - native-open 独占唯一一次 decodeURIComponent, 随后交给 URLSearchParams 做
 *   application/x-www-form-urlencoded 编码 (含 token);
 * - 缺失/非法 target 一律 fail-closed: 清空面板 target, 禁用全部 action,
 *   不发送请求, 不复用旧 target。
 *
 * 浏览状态模型 (浏览视图控件计划 issue 04):
 * - tree/list/grid 只有一个真源 state.view; 独立阅读页 state.view === null,
 *   不是第四种浏览视图. 显示偏好 (cb-browse-view / cb-sort / cb-hidden) 只在
 *   用户操作控件时写入; 显式 URL、初始化规范化和历史恢复绝不回写偏好.
 * - 有效恢复快照优先; 冷访问按显式 URL > 偏好 > 默认. 内容安装键包含上下文、
 *   查询与显示规则版本 (文档正文不含规则), 只有一致且成功安装才可跳过重取;
 *   过期成功与过期错误一律丢弃, 不加自动重试.
 * - managed 历史快照 (history.state.cb) 保存表面/视图/路径/查询/hash/规则以及
 *   打开与选中条目各自的 encoded identity; 绝不含 token 或正文, 分享 URL 不含
 *   layout. 同一位置控件变化 replace 当前历史项, 真实导航只新增一个目标项.
 *
 * 测试钩子: 纯函数在 CommonJS 环境 (Node/Bun) 下经 module.exports 暴露,
 * 浏览器环境无任何全局污染。
 */
(() => {
  'use strict';

  /* ---------- 纯函数: 可独立于 DOM 测试 ---------- */

  // 规范 ASCII transport: unreserved / 服务端原样保留的 RFC 3986 sub-delims
  // !$()* / %HH / '/'; 无前导斜杠; 无空段; 无字面或 percent-encoded 的 dot
  // segment (WHATWG 单/双 dot 的全部六种拼写, 大小写不敏感: `.`, `..`, `%2e`,
  // `%2e%2e`, `.%2e`, `%2e.` — URL 解析器都会折叠并改写目标, 必须 fail-closed;
  // 服务端对字面 `%` 编码为 `%25`, 故字面文件名 `%2e%2e` 传输为 `%252e%252e`,
  // 不在拒绝集)。其余 sub-delims (+ , ; = & ') 与 '#' '?' 空格等一律被服务端
  // 编码, raw 形态必须拒绝 (fail-closed)。
  // 空串合法 (root 的 encodedPath 是空字符串而不是属性缺失); 缺失/非法 → false。
  function isValidEncodedPath(value) {
    if (typeof value !== 'string') return false;
    if (value.length === 0) return true;
    if (value[0] === '/' || value[value.length - 1] === '/') return false;
    if (/[^A-Za-z0-9\-._~%!$()*/]/.test(value)) return false;
    if (/%(?![0-9A-Fa-f]{2})/.test(value)) return false;
    return value.split('/').every(function (seg) {
      return (
        seg.length > 0 &&
        // 整段恰好是 1~2 个 dot 单元 (`.` 或大小写不敏感 `%2e`) 的组合:
        // 单 dot `.`/`%2e`; 双 dot `..`/`%2e%2e`/`.%2e`/`%2e.`。混合拼写同样
        // 被 WHATWG 折叠, 必须与纯拼写一并拒绝; `%252e` 不匹配 (3 字符单元)。
        !/^(?:\.|\.\.|%2e|%2e%2e|\.%2e|%2e\.)$/i.test(seg)
      );
    });
  }
  const TRANSPORT_LITERAL_ASCII = /^[A-Za-z0-9\-._~!$()*]$/;

  function hexByte(byte) {
    return '%' + byte.toString(16).toUpperCase().padStart(2, '0');
  }

  // Canonical comparison key only: never used to construct an action URL.
  // Literal '/' is the only segment separator; encoded '/' is rejected.
  function canonicalTransportKey(value) {
    if (typeof value !== 'string') return null;
    if (value.length === 0) return '';
    if (value[0] === '/' || value[value.length - 1] === '/') return null;
    const out = [];
    for (const rawSegment of value.split('/')) {
      if (rawSegment.length === 0) return null;
      let segment = '';
      for (let i = 0; i < rawSegment.length; ) {
        const ch = rawSegment[i];
        if (ch === '%') {
          if (i + 2 >= rawSegment.length) return null;
          const pair = rawSegment.slice(i + 1, i + 3);
          if (!/^[0-9A-Fa-f]{2}$/.test(pair)) return null;
          const byte = parseInt(pair, 16);
          if (byte === 0x2f) return null;
          const ascii = byte < 0x80 ? String.fromCharCode(byte) : null;
          segment += ascii && TRANSPORT_LITERAL_ASCII.test(ascii) ? ascii : hexByte(byte);
          i += 3;
          continue;
        }
        const cp = rawSegment.codePointAt(i);
        if (cp >= 0xd800 && cp <= 0xdfff) return null; // 孤立代理项: 合法标量拒绝
        const scalar = String.fromCodePoint(cp);
        const bytes = new TextEncoder().encode(scalar);
        if (bytes.length === 1 && TRANSPORT_LITERAL_ASCII.test(scalar)) {
          segment += scalar;
        } else {
          bytes.forEach(function (byte) { segment += hexByte(byte); });
        }
        i += scalar.length;
      }
      if (segment === '.' || segment === '..') return null;
      out.push(segment);
    }
    return out.join('/');
  }

  // 单一 action URL 出口: 不 decode encodedPath, 直接放进 URL pathname;
  // 模式只经 URL.searchParams 添加 (fragment=1 / download=1); Full 无 query。
  // 非法 encodedPath 返回 null (fail-closed)。
  // baseUrl 仅供测试注入 origin; 浏览器默认 location.origin。
  function actionUrl(encodedPath, mode, baseUrl) {
    if (!isValidEncodedPath(encodedPath)) return null;
    const origin = baseUrl !== undefined ? baseUrl : location.origin;
    const url = new URL('/' + encodedPath, origin);
    if (mode === 'fragment') {
      url.searchParams.set('fragment', '1');
    } else if (mode === 'download') {
      url.searchParams.set('download', '1');
    }
    return url;
  }

  // native-open 独占 seam: 恰好一次 decodeURIComponent, 再经 URLSearchParams
  // 做 form 编码 (path + token)。decode 失败返回 null (清状态, 不发送请求)。
  function nativeOpenParams(encodedPath, token) {
    if (!isValidEncodedPath(encodedPath)) return null;
    let decoded;
    try {
      decoded = decodeURIComponent(encodedPath);
    } catch (err) {
      return null;
    }
    const params = new URLSearchParams();
    params.set('path', decoded);
    params.set('token', token);
    return params;
  }

  // Escape 决策 (DOM 无关 seam): 搜索框聚焦 → 先失焦 (blur-search 优先);
  // 否则面板打开 → 关闭; 两者皆否 → 不处理 (null, 调用方不得取消默认行为)。
  // 必须在通用 editable-target 守卫之前调用, 否则聚焦在搜索框时 Escape 会被
  // 守卫直接吞掉。
  function escapeAction(searchFocused, panelOpen) {
    if (searchFocused) return 'blur-search';
    if (panelOpen) return 'close-panel';
    return null;
  }

  // PDF 判定 (DOM 无关 seam): kind 必须为 'file' 且编码后缀匹配 .pdf
  // (大小写不敏感)。编码串后缀可直接判定 — percent-encoding 从不编码字面字母
  // 与点, 扩展名只可能来自末段 (`dir.pdf/file.txt` 以 .txt 结尾); decode 保持
  // native-open 独占 seam, 此处绝不 decode。
  function isPdfFile(encodedPath, kind) {
    return kind === 'file' && /\.pdf$/i.test(encodedPath);
  }

  // 选择后的 panel target 决策 (DOM 无关 seam): 选中非 actionable 条目
  // (selectedEncoded === null) 必须立即清空旧 target, 绝不复用 stale target;
  // actionable 选择保持现有 panel target (只有预览安装 target)。
  function panelTargetForSelection(selectedEncoded, currentTarget) {
    return selectedEncoded === null ? null : currentTarget;
  }

  // 通用单调 generation 守卫 (预览/内容区共用) (DOM 无关 seam): begin() 使旧 generation 过期;
  // isCurrent(gen) 判定在途响应/错误是否仍属于最新请求; invalidate() 使一切
  // 在途响应/错误失效 (面板关闭时调用, 迟到的响应/错误不得安装任何状态)。
  function createGenerationGuard() {
    let gen = 0;
    return {
      begin: function () {
        return ++gen;
      },
      isCurrent: function (g) {
        return g === gen;
      },
      invalidate: function () {
        gen++;
      },
    };
  }

  // fragment 响应严格读取 (预览面板与内容区共用): ok + text/html 校验后返回文本.
  function readFragmentResponse(res) {
    if (!res.ok) throw new Error('HTTP ' + res.status);
    const ct = res.headers.get('Content-Type') || '';
    if (!/text\/html/i.test(ct)) throw new Error('响应不是 HTML');
    return res.text();
  }

  // 树宽拖拽手势所有权: 仅主指针左键可在无在途手势时开始 (右键/中键/副指针/
  // 多指一律拒绝); move/end 只认持有手势的 pointerId, 其他指针不得移动或
  // 结束在途拖拽. isPrimary 省略视同主指针 (合成事件/键盘可达).
  function canStartTreeResize(event, activePointerId) {
    return activePointerId === null && event.button === 0 && event.isPrimary !== false;
  }
  function ownsTreeResize(event, activePointerId) {
    return activePointerId !== null && event.pointerId === activePointerId;
  }

  /* ---------- 显示规则与偏好 (纯 seam) ---------- */

  const SORT_COLUMNS = ['Name', 'Type', 'Size', 'LastModified', 'LastAccess', 'Creation'];
  const SORT_ORDERS = ['Asc', 'Desc'];
  const DEFAULT_SORT = 'Name:Asc';
  const DEFAULT_VIEW = 'list';
  const DEFAULT_HIDDEN = false;

  // sort 精确语法 `Column:Order` (大小写敏感; 非法值服务端 404, 客户端不入状态).
  function isValidSort(value) {
    if (typeof value !== 'string') return false;
    const parts = value.split(':');
    if (parts.length !== 2) return false;
    return SORT_COLUMNS.indexOf(parts[0]) >= 0 && SORT_ORDERS.indexOf(parts[1]) >= 0;
  }

  function isValidHidden(value) {
    return value === '0' || value === '1';
  }

  // v 键循环: list → grid → tree → list (未知视同一 list 的下一步).
  function nextBrowseView(view) {
    if (view === 'list') return 'grid';
    if (view === 'grid') return 'tree';
    return 'list';
  }

  // 父目录 encoded path: 'a' → '', 'a/b' → 'a'; root/非法 → null.
  function parentEncodedPath(encodedPath) {
    if (!isValidEncodedPath(encodedPath) || encodedPath === '') return null;
    const idx = encodedPath.lastIndexOf('/');
    return idx < 0 ? '' : encodedPath.slice(0, idx);
  }

  // 隐藏项判定 (Q3): 名称以 '.' 开头. 编码路径接受原样 '.' (unreserved) 与
  // '%2E'/'%2e' 编码形态; 字面 '%' 在 transport 编码为 %25, 不会误判.
  function isHiddenName(name) {
    return typeof name === 'string' && name[0] === '.';
  }
  function isHiddenEncodedPath(encodedPath) {
    if (typeof encodedPath !== 'string' || encodedPath === '') return false;
    const idx = encodedPath.lastIndexOf('/');
    const last = idx < 0 ? encodedPath : encodedPath.slice(idx + 1);
    return last[0] === '.' || /^%2e/i.test(last);
  }

  // 最近可见条目索引 (Q17 隐藏焦点回退, DOM 无关 seam): 从 fromIndex 起优先
  // 向后找最近可见项, 其后向前找; 全部不可见返回 -1 (空列表落到容器).
  function nearestVisibleIndex(visible, fromIndex) {
    if (!Array.isArray(visible) || visible.length === 0) return -1;
    const start = Math.max(0, Math.min(Math.round(fromIndex), visible.length - 1));
    for (let i = start; i < visible.length; i++) {
      if (visible[i]) return i;
    }
    for (let i = start - 1; i >= 0; i--) {
      if (visible[i]) return i;
    }
    return -1;
  }

  // 树分支加载提交决策 (DOM 无关 seam):
  // - 'drop': 旧壳响应 — 不写缓存、不落 DOM、不报错;
  // - 'stale': 旧显示规则版本的响应/错误 — 全部丢弃 (与缓存一并失效);
  // - 'cache': 当前版本但导航所有权已陈旧 — 只共享缓存结果, 不提交 DOM/展开;
  // - 'install': 提交缓存并安装 DOM.
  function treeLoadDecision(shellCurrent, versionCurrent, navRequired, navCurrent) {
    if (!shellCurrent) return 'drop';
    if (!versionCurrent) return 'stale';
    if (navRequired && !navCurrent) return 'cache';
    return 'install';
  }

  /* ---------- managed 历史快照 (纯 seam) ---------- */

  const SNAPSHOT_VERSION = 1;
  const SNAPSHOT_SURFACES = ['directory', 'search', 'reading'];
  const SNAPSHOT_VIEWS = ['tree', 'list', 'grid'];

  // 快照结构校验 (DOM 无关 seam): 表面/视图/路径/查询/hash/规则/打开与选中
  // identity; 绝不含 token 或正文. 独立阅读无浏览视图 (view === null), 但树壳
  // 可以覆盖在阅读目标上 (view === 'tree').
  function isValidSnapshot(value) {
    if (!value || typeof value !== 'object') return false;
    if (value.v !== SNAPSHOT_VERSION) return false;
    if (SNAPSHOT_SURFACES.indexOf(value.surface) < 0) return false;
    if (value.view !== null && SNAPSHOT_VIEWS.indexOf(value.view) < 0) return false;
    if (!isValidSort(value.sort)) return false;
    if (!isValidHidden(String(value.hidden))) return false;
    if (typeof value.hash !== 'string') return false;
    // 阅读位置 (独立阅读表面的窗口滚动, px): 可选字段. 存在时必须是有限非负数;
    // 旧快照没有此键 — 缺失视为无恢复意图, 不视为失效.
    if (
      value.scroll !== undefined &&
      value.scroll !== null &&
      !(typeof value.scroll === 'number' && Number.isFinite(value.scroll) && value.scroll >= 0)
    ) {
      return false;
    }
    if (value.opened !== null && !isValidEncodedPath(value.opened)) return false;
    if (value.openedKind !== null && value.openedKind !== 'reading' && value.openedKind !== 'directory') return false;
    if ((value.opened === null) !== (value.openedKind === null)) return false;
    if (value.selected !== null && !isValidEncodedPath(value.selected)) return false;
    if (value.surface === 'search') {
      if (typeof value.q !== 'string') return false;
      if (value.path !== '') return false;
      if (value.view === null) return false;
      return true;
    }
    if (value.q !== null && value.q !== undefined) return false;
    if (!isValidEncodedPath(value.path)) return false;
    if (value.surface === 'reading') {
      // 阅读目标的路径必须非空; 只允许无视图或树壳.
      if (value.path === '') return false;
      return value.view === null || value.view === 'tree';
    }
    // directory 表面始终有活跃浏览视图.
    return value.view !== null;
  }

  /* ---------- DOM 小工具 ---------- */

  function $(sel, root) {
    return (root || document).querySelector(sel);
  }

  function $$(sel, root) {
    return Array.from((root || document).querySelectorAll(sel));
  }

  // 能力仅来自合法完整页; 进入树壳清除, 不进入快照或浏览器存储.
  let nativeToken = null;

  /* ---------- 偏好读写与一次性迁移 ---------- */

  const PREF_VIEW = 'cb-browse-view'; // 'tree' | 'list' | 'grid'
  const PREF_SORT = 'cb-sort'; // 'Column:Order'
  const PREF_HIDDEN = 'cb-hidden'; // '1' | '0'
  const LEGACY_MODE_KEY = 'cb-mode'; // 旧二级模式偏好: 'tree'
  const LEGACY_VIEW_KEY = 'cb-view'; // 旧列表/网格偏好: 'list' | 'grid'

  function readPref(key) {
    try {
      return localStorage.getItem(key);
    } catch (err) {
      return null; // 隐私模式/存储禁用: 仅内存态可用
    }
  }

  function writePref(key, value) {
    try {
      localStorage.setItem(key, value);
    } catch (err) {
      /* 持久化失败忽略 */
    }
  }

  function removePref(key) {
    try {
      localStorage.removeItem(key);
    } catch (err) {
      /* 持久化失败忽略 */
    }
  }

  // 有效视图偏好解析 (只读): 新键优先; 否则按旧键一次性映射 (旧 tree 优先,
  // 否则 list/grid); 无偏好 → 默认列表. migrated 表示结果需要落盘到新键.
  function resolveViewPref() {
    const stored = readPref(PREF_VIEW);
    if (stored === 'tree' || stored === 'list' || stored === 'grid') {
      return { view: stored, migrated: false };
    }
    const legacyMode = readPref(LEGACY_MODE_KEY);
    const legacyView = readPref(LEGACY_VIEW_KEY);
    let view = DEFAULT_VIEW;
    if (legacyMode === 'tree') {
      view = 'tree';
    } else if (legacyView === 'grid') {
      view = 'grid';
    }
    return { view: view, migrated: legacyMode !== null || legacyView !== null };
  }

  // 一次性迁移旧 cb-mode / cb-view 到三态 cb-browse-view: 旧 tree 优先, 否则
  // 保留 grid/list; 写入新键后删除旧键, 不保留长期兼容别名.
  function loadViewPref() {
    const resolved = resolveViewPref();
    if (resolved.migrated) {
      writePref(PREF_VIEW, resolved.view);
      removePref(LEGACY_MODE_KEY);
      removePref(LEGACY_VIEW_KEY);
    }
    return resolved.view;
  }

  function loadSortPref() {
    const value = readPref(PREF_SORT);
    return isValidSort(value) ? value : DEFAULT_SORT;
  }

  function loadHiddenPref() {
    const value = readPref(PREF_HIDDEN);
    return isValidHidden(value) ? value === '1' : DEFAULT_HIDDEN;
  }

  function persistView(view) {
    writePref(PREF_VIEW, view);
  }
  function persistSort(sort) {
    writePref(PREF_SORT, sort);
  }
  function persistHidden(hidden) {
    writePref(PREF_HIDDEN, hidden ? '1' : '0');
  }

  /* ---------- 浏览状态 ---------- */

  // kind: 内容区语义 (directory|search|reading); view: 活跃浏览视图或 null;
  // path: 内容区目标的编码 identity ('' 为服务根; search 恒为 '');
  // q: 搜索查询 (仅 search); opened/openedKind: 预览区打开条目; selected: 选中条目.
  const state = {
    kind: 'directory',
    view: null,
    path: '',
    q: null,
    rules: { sort: DEFAULT_SORT, hidden: DEFAULT_HIDDEN },
    opened: null,
    openedKind: null,
    selected: null,
  };

  // 显示规则版本: 每次规则变化递增; 树缓存/空分支状态/在途结果都按版本提交.
  let optionsVersion = 0;
  let initialized = false;
  // 本次文档加载是否消费了有效 managed 快照 (冷访问才回落共享偏好).
  let bootSnapshot = false;

  function isBrowseView() {
    return state.view === 'tree' || state.view === 'list' || state.view === 'grid';
  }

  // 服务器页面语义 (body data-cb-surface); 缺失时不激活浏览增强 (fail-closed).
  function bodySurface() {
    const body = document.body;
    const explicit = body && body.dataset ? body.dataset.cbSurface : null;
    if (explicit === 'directory' || explicit === 'search' || explicit === 'reading') {
      return explicit;
    }
    return null;
  }

  // 当前 URL 的路径 identity (canonical 编码, 无前导斜杠; 非法 → null).
  function locationEncodedPath() {
    const pathname = location.pathname;
    if (pathname === '/' || pathname === '') return '';
    const rel = pathname.slice(1).replace(/\/+$/, '');
    return isValidEncodedPath(rel) ? rel : null;
  }

  function locationPathname() {
    const pathname = location.pathname.replace(/\/+$/, '');
    return pathname === '' ? '/' : pathname;
  }

  function isSearchLocation() {
    return locationPathname() === '/__/search';
  }

  function urlParams() {
    return new URL(location.href).searchParams;
  }

  // 显式 URL 规则: 缺失/非法返回 null (非法值服务端已 404, 不会到达这里).
  function explicitUrlRules() {
    const params = urlParams();
    const sort = params.get('sort');
    const hidden = params.get('hidden');
    return {
      sort: isValidSort(sort) ? sort : null,
      hidden: isValidHidden(hidden) ? hidden === '1' : null,
    };
  }

  // 浏览上下文 URL: 只带 q (search) 与 sort/hidden, 绝不携带 layout/fragment/
  // download/view 等传输模式; hash 由调用方决定 (同文档延续, 新文档丢弃).
  // sort/hidden 手工拼装: 保持服务端 `Column:Order` 字面冒号形式 (URLSearchParams
  // 会把 ':' 编码为 %3A); q 经 form 语义编码 (与搜索表单提交一致).
  function contextUrl(kind, encodedPath, query, hash) {
    const base = kind === 'search' ? '/__/search' : '/' + encodedPath;
    const parts = [];
    if (kind === 'search') {
      const qParams = new URLSearchParams();
      qParams.set('q', query || '');
      parts.push(qParams.toString());
    }
    parts.push('sort=' + state.rules.sort);
    parts.push('hidden=' + (state.rules.hidden ? '1' : '0'));
    return new URL(base + '?' + parts.join('&') + (hash || ''), location.origin);
  }

  // UI 导航沿用有效条目规则; transport identity 始终由纯 actionUrl 构造.
  function browseActionUrl(encodedPath, mode) {
    const url = actionUrl(encodedPath, mode);
    if (url && mode !== 'download') {
      url.search += (url.search ? '&' : '') +
        'sort=' + state.rules.sort + '&hidden=' + (state.rules.hidden ? '1' : '0');
    }
    return url;
  }

  // 当前 URL 就地更新为有效规则 (保留其他参数与 hash); 用于同位置控件变化.
  function currentEntryUrlWithRules() {
    const url = new URL(location.href);
    const rest = url.search
      ? url.search
          .slice(1)
          .split('&')
          .filter(function (pair) {
            const key = pair.split('=')[0];
            return key !== 'sort' && key !== 'hidden' && key !== 'q';
          })
      : [];
    if (state.kind === 'search') {
      const qParams = new URLSearchParams();
      qParams.set('q', state.q || '');
      rest.unshift(qParams.toString());
    }
    rest.push('sort=' + state.rules.sort);
    rest.push('hidden=' + (state.rules.hidden ? '1' : '0'));
    url.search = rest.join('&'); // search setter 不编码 ':'/'+' (保留字面形式)
    return url;
  }

  // 独立阅读位置 (px): 只有阅读表面 (阅读目标且无浏览视图) 记录窗口滚动 —
  // 树壳下窗口不滚动 (树与内容区各有滚动容器), 其它表面与该导航无关.
  function readingScrollY() {
    if (state.kind !== 'reading' || state.view !== null) return null;
    const y = window.scrollY;
    return Number.isFinite(y) && y > 0 ? Math.round(y) : 0;
  }

  // 恢复窗口滚动位置: 立即应用一次, 文档未完全加载时在 load 后再应用一次
  // (图片/字体撑高文档后 scrollTo 不被截断到旧高度). 显式 instant: 文档页 CSS
  // 的 scroll-behavior: smooth 不得把恢复变成动画 (恢复必须是确定落点).
  function restoreReadingScroll(y) {
    if (typeof y !== 'number' || !Number.isFinite(y) || y <= 0) return;
    const apply = function () {
      window.scrollTo({ top: y, left: 0, behavior: 'instant' });
    };
    apply();
    if (document.readyState !== 'complete') {
      window.addEventListener('load', apply, { once: true });
    }
  }

  function snapshotNow() {
    return {
      v: SNAPSHOT_VERSION,
      surface: state.kind,
      view: state.view,
      path: state.path,
      q: state.kind === 'search' ? state.q : null,
      hash: location.hash,
      scroll: readingScrollY(),
      sort: state.rules.sort,
      hidden: state.rules.hidden ? 1 : 0,
      opened: state.opened,
      openedKind: state.openedKind,
      selected: state.selected,
    };
  }

  // 同位置控件变化: 只更新当前历史项 (URL 与快照), 不新增历史, 不写偏好.
  function replaceCurrentEntry(url) {
    history.replaceState({ cb: snapshotNow() }, '', url ? url.href : location.href);
  }

  // 真实路径/搜索导航: 新增一个携带快照的目标项.
  function pushEntry(url) {
    history.pushState({ cb: snapshotNow() }, '', url.href);
  }

  function readSnapshot() {
    const raw = history.state;
    const value = raw && typeof raw === 'object' ? raw.cb : null;
    return isValidSnapshot(value) ? value : null;
  }

  // 快照只有在与当前 URL 上下文一致时才可消费 (reload/popstate/BFCache).
  function snapshotMatchesLocation(snapshot) {
    if (snapshot.surface === 'search') {
      if (!isSearchLocation()) return false;
      return (urlParams().get('q') || '') === (snapshot.q || '');
    }
    const urlPath = locationEncodedPath();
    if (urlPath === null) return false;
    const a = canonicalTransportKey(snapshot.path);
    const b = canonicalTransportKey(urlPath);
    return a !== null && a === b;
  }

  function applySnapshot(snapshot) {
    state.kind = snapshot.surface;
    state.view = snapshot.view;
    state.path = snapshot.path;
    state.q = snapshot.surface === 'search' ? snapshot.q : null;
    state.rules = { sort: snapshot.sort, hidden: snapshot.hidden === 1 };
    state.opened = snapshot.opened;
    state.openedKind = snapshot.openedKind;
    state.selected = snapshot.selected;
  }

  // 完整页交接 (补能力 / 视图切换): 目标路径与当前位置相同 → replace 当前项;
  // 不同 → 只新增一个携带快照的目标项; 随后整页加载恢复能力.
  function handoff(target, view) {
    state.view = view;
    state.kind = target.kind;
    state.path = target.path;
    state.q = target.kind === 'search' ? state.q : null;
    state.opened = target.opened;
    state.openedKind = target.openedKind || null;
    state.selected = target.locate;
    const sameLocation = target.sameLocation === true;
    const url = contextUrl(
      target.kind,
      target.path,
      target.kind === 'search' ? state.q : null,
      sameLocation ? location.hash : ''
    );
    if (sameLocation) {
      history.replaceState({ cb: snapshotNow() }, '', url.href);
    } else {
      history.pushState({ cb: snapshotNow() }, '', url.href);
    }
    location.reload();
  }

  /* ---------- 工具区控件 (视图 / 隐藏 / 排序) ---------- */

  function viewButtons() {
    return $$('[data-cb-view]');
  }

  function hiddenToggle() {
    return $('[data-cb-hidden-toggle]');
  }

  function sortControl() {
    return $('.cb-sort');
  }

  // 排序入口 = 顶部 12 个菜单链接 + 表头快捷入口: 只认可交互 anchor (body 与
  // fragment wrapper 用同名 data-cb-sort 携带元数据, 绝不能被当成控件).
  function sortLinks() {
    return $$('a[data-cb-sort]');
  }

  // 搜索表单 = 共享工具区 + 独立阅读导航的桌面/移动副本 (同一 action, 各自
  // 独立 id): 显示规则字段逐份同步, 任一可见表单提交都携带当前规则.
  function searchForms() {
    return $$('form.cb-search');
  }

  // 控件状态同步: 视图/隐藏/排序控件只存在于浏览表面 (共享工具区, 或树壳克隆
  // 的工具区; 独立阅读导航不承载这些控件). 客户端启用 = 清除服务端
  // aria-disabled, 并让每个入口都携带有效显示规则.
  function syncControlState() {
    viewButtons().forEach(function (btn) {
      btn.removeAttribute('aria-disabled');
      btn.setAttribute('data-cb-tip', btn.getAttribute('aria-label'));
      const active = btn.getAttribute('data-cb-view') === state.view;
      btn.setAttribute('aria-pressed', active ? 'true' : 'false');
      // 视图入口的渐进增强 href (列表 anchor) 延续有效显示规则.
      if (btn.tagName === 'A') {
        btn.setAttribute(
          'href',
          hrefWithRules(btn.getAttribute('href') || location.href, {
            sort: state.rules.sort,
            hidden: state.rules.hidden,
          })
        );
      }
    });
    const toggle = hiddenToggle();
    if (toggle) {
      toggle.removeAttribute('aria-disabled');
      toggle.setAttribute('aria-pressed', state.rules.hidden ? 'true' : 'false');
      toggle.setAttribute(
        'data-cb-tip',
        state.rules.hidden ? '不显示隐藏项' : '显示隐藏项'
      );
      if (toggle.tagName === 'A') {
        toggle.setAttribute(
          'href',
          hrefWithRules(toggle.getAttribute('href') || location.href, {
            sort: state.rules.sort,
            hidden: !state.rules.hidden,
          })
        );
      }
    }
    const control = sortControl();
    if (control) {
      control.removeAttribute('aria-disabled');
      const summary = control.querySelector('summary');
      if (summary) summary.setAttribute('data-cb-tip', '排序');
    }
    let currentLabel = null;
    sortLinks().forEach(function (link) {
      link.removeAttribute('aria-disabled');
      const value = link.getAttribute('data-cb-sort');
      // 表头链接表示下一次排序动作; 当前摘要只能取工具区内的选项标签.
      const current = !!control && control.contains(link) && value === state.rules.sort;
      if (current) {
        link.setAttribute('aria-current', 'true');
        currentLabel = link.textContent ? link.textContent.trim() : null;
      } else {
        link.removeAttribute('aria-current');
      }
      // 每个排序入口都是真实 anchor: href 延续有效显示规则 (保留 q).
      link.setAttribute(
        'href',
        hrefWithRules(link.getAttribute('href') || location.href, {
          sort: value,
          hidden: state.rules.hidden,
        })
      );
    });
    const sortState = $('[data-cb-sort-state]');
    if (sortState && currentLabel) sortState.textContent = currentLabel;
    searchForms().forEach(function (form) {
      const sortField = form.querySelector('input[name="sort"]');
      const hiddenField = form.querySelector('input[name="hidden"]');
      if (sortField) sortField.value = state.rules.sort;
      if (hiddenField) hiddenField.value = state.rules.hidden ? '1' : '0';
    });
    syncReadingNav();
  }

  // 在既有 href 上就地替换 sort/hidden (保留 q 与其他必要字段; 非法 href 原样).
  // 手工拼装保持服务端 `Column:Order` 字面形式, 不重新编码其余参数.
  function hrefWithRules(href, rules) {
    if (typeof href !== 'string' || href === '') return href;
    let url;
    try {
      url = new URL(href, location.href);
    } catch (err) {
      return href;
    }
    if (url.origin !== location.origin) return href;
    const pairs = url.search
      ? url.search
          .slice(1)
          .split('&')
          .filter(function (pair) {
            const key = pair.split('=')[0];
            return key !== 'sort' && key !== 'hidden';
          })
      : [];
    if (rules.sort !== undefined && rules.sort !== null) {
      pairs.push('sort=' + rules.sort);
    }
    if (rules.hidden !== undefined && rules.hidden !== null) {
      pairs.push('hidden=' + (rules.hidden ? '1' : '0'));
    }
    return url.pathname + (pairs.length ? '?' + pairs.join('&') : '') + url.hash;
  }

  /* ---------- 独立阅读导航 (阅读页) ---------- */

  // 独立阅读表面 = 阅读目标且无浏览视图 (树壳下的阅读目标是浏览表面).
  function isReadingSurface() {
    return state.kind === 'reading' && state.view === null;
  }

  function readingNav() {
    return $('header.cb-reading-nav');
  }

  // 依赖 JS 的动作 (文件树/复制链接) 在服务端保持 hidden; 只有合法 identity 的
  // 阅读页才启用 — identity 缺失时 init 已 fail-closed, 控件绝不解除 hidden.
  function enableReadingActions() {
    $$('[data-cb-reading-tree], [data-cb-reading-copy]').forEach(function (btn) {
      btn.hidden = false;
    });
  }

  // 阅读导航的规则延续: 面包屑/所在目录/下载等真实链接保持当前 sort/hidden
  // (服务端渲染时的规则可能已被偏好或显式 URL 覆盖).
  function syncReadingNav() {
    const nav = readingNav();
    if (!nav) return;
    $$('a[href]', nav).forEach(function (link) {
      const href = link.getAttribute('href');
      if (!href || href.charAt(0) === '#') return;
      link.setAttribute(
        'href',
        hrefWithRules(href, { sort: state.rules.sort, hidden: state.rules.hidden })
      );
    });
  }

  // 阅读导航高度 (sticky 顶栏 / scroll-padding / TOC 偏移): ResizeObserver
  // 同步根变量, CSS 兜底高度在变量缺席时仍可用.
  function installReadingNavHeight(nav) {
    const update = function () {
      document.documentElement.style.setProperty(
        '--cb-reading-nav-height',
        nav.offsetHeight + 'px'
      );
    };
    new ResizeObserver(update).observe(nav);
    update();
  }

  // 阅读搜索 (Q8): 原生 details 展开 + 原生表单提交; 展开时聚焦输入, 收起时把
  // 焦点交回入口; Escape 关闭. 输入/可编辑目标永不抢键 (全局处理器已先放行).
  function installReadingSearch() {
    $$('details.cb-reading-search').forEach(function (details) {
      details.addEventListener('toggle', function () {
        const input = details.querySelector('form.cb-search input[name="q"]');
        if (details.open) {
          if (input) input.focus();
          return;
        }
        if (details.contains(document.activeElement)) {
          const summary = details.querySelector('summary');
          if (summary) summary.focus();
        }
      });
      details.addEventListener('keydown', function (event) {
        if (event.key !== 'Escape' || !details.open) return;
        event.preventDefault();
        details.open = false; // toggle 处理器把焦点交回入口
      });
    });
  }

  // 阅读搜索展开并聚焦: 宽窄屏各有一份, 只打开并聚焦当前显示的那一份 (不复制
  // 输入, 表单仍是唯一真源). 窄屏那份藏在"更多"弹层里 — 宿主弹层先展开, 绝不
  // 把焦点交给不可见控件.
  function focusReadingSearch() {
    const groups = $$('details.cb-reading-search');
    if (groups.length === 0) return false;
    let group = groups.find(function (el) {
      return el.getClientRects().length > 0;
    });
    if (!group) {
      group =
        groups.find(function (el) {
          return el.closest('.cb-reading-more') !== null;
        }) || groups[0];
    }
    // 关闭的 <details> 后代仍可能有 layout rect; 不能据此跳过宿主展开。
    const host = group.closest('details.cb-reading-more');
    if (host) host.open = true;
    group.open = true;
    const input = group.querySelector('form.cb-search input[name="q"]');
    if (input) input.focus();
    return true;
  }

  // '/' 焦点入口: 阅读表面聚焦阅读搜索, 其余表面沿用共享工具区搜索框.
  function focusSearchInput() {
    if (isReadingSurface() && focusReadingSearch()) return true;
    const input = topbarSearchInput();
    if (!input) return false;
    input.focus();
    return true;
  }

  function setReadingStatus(text) {
    $$('[data-cb-reading-status]').forEach(function (el) {
      el.textContent = text;
    });
  }

  // 复制链接 (Q7): 复制当前页面真实 URL (含已有章节锚点, 绝不写入滚动高亮的
  // 章节); 剪贴板不可用或被拒绝时展开可手动选择的完整链接, 不显示虚假成功.
  function copyReadingLink() {
    const url = location.href;
    const fallback = $('[data-cb-reading-copy-fallback]');
    const fail = function () {
      if (fallback) {
        const input = fallback.querySelector('input');
        if (input) input.value = url;
        fallback.hidden = false;
        if (input) {
          input.focus();
          input.select();
        }
      }
      setReadingStatus('无法自动复制：请手动复制完整链接');
    };
    if (!navigator.clipboard || typeof navigator.clipboard.writeText !== 'function') {
      fail();
      return;
    }
    navigator.clipboard.writeText(url).then(
      function () {
        if (fallback) fallback.hidden = true;
        setReadingStatus('已复制链接');
      },
      fail
    );
  }

  // 定位 identity: 当前文件被隐藏规则过滤 (hidden=0 时不显示隐藏项) 时不定位 —
  // 只进入所在目录, 不开启隐藏项、不注入过滤例外、不打开替代条目.
  function readingLocateIdentity() {
    if (!isValidEncodedPath(state.path) || state.path === '') return null;
    return isHiddenEncodedPath(state.path) && !state.rules.hidden ? null : state.path;
  }

  // 所在目录 (Q4/Q11/Q12): 按保存的浏览视图 (无偏好 → 列表) 延续当前显示规则
  // 进入父目录; 只定位当前文件 (被隐藏规则过滤时不定位, 不开启隐藏项、不注入
  // 过滤例外、不打开替代条目), 不打开预览, 不改写显示偏好.
  function navigateReadingParent() {
    if (!isReadingSurface() || state.path === '') return;
    const parent = parentEncodedPath(state.path);
    if (parent === null) return; // 非法 identity: 不猜目标
    replaceCurrentEntry(); // 阅读项记录当前滚动 (后退回到阅读页时恢复实际位置)
    handoff(
      {
        kind: 'directory',
        path: parent,
        locate: readingLocateIdentity(),
        opened: null,
        openedKind: null,
        sameLocation: false,
      },
      resolveViewPref().view
    );
  }

  // 在文件树中浏览 (Q6/Q13): 当前阅读历史项先记录进入树前的真实阅读位置, 再
  // 只新增一个树历史项 (同一 URL 允许) — 当前文件仍是阅读对象, 树展开并选中
  // 它; 进入树只写视图偏好, 历史恢复不回写偏好.
  function enterReadingTree() {
    if (!isReadingSurface() || state.path === '') return;
    if (!isValidEncodedPath(state.path)) return;
    replaceCurrentEntry(); // 阅读项记录当前滚动 (back 恢复实际位置而非锚点)
    state.view = 'tree';
    state.selected = state.path;
    persistView('tree');
    pushEntry(contextUrl('reading', state.path, null, location.hash));
    ensureTreeShell(true);
  }

  // 工具区交接: 树壳使用既有浏览工具区 (克隆惰性模板), 不叠加独立阅读导航;
  // 模板缺失 (旧页) 时不隐藏导航, 避免树壳失去全部控件.
  function installBrowseTopbarReplacement() {
    const nav = readingNav();
    if (!nav) return;
    if ($('header.cb-topbar')) {
      nav.hidden = true; // 已交接 (幂等)
      return;
    }
    const template = document.getElementById('cb-browse-topbar');
    if (!template || !template.content) return;
    nav.before(template.content.cloneNode(true));
    nav.hidden = true;
    // 克隆出来的工具区必须重新量一次边界变量: 窄屏抽屉/遮罩以
    // --cb-topbar-bottom 定位 (回退 0px 会盖住工具区).
    const topbar = $('header.cb-topbar');
    if (topbar) installTopbarBounds(topbar);
  }

  // 阅读导航安装: 只在合法 identity 的阅读页调用 (fail-closed 分支直接返回,
  // 不启用任何依赖 JS 的能力).
  function installReadingNav() {
    const nav = readingNav();
    if (!nav) return;
    enableReadingActions();
    installReadingSearch();
    installReadingNavHeight(nav);
  }

  /* ---------- 列表/网格条目与选中 ---------- */

  const ENTRY_SELECTOR = '[data-browser-entry]';
  const ACTION_SELECTOR = '[data-cb-action]';
  const PANEL_SELECTOR = '#cb-preview';
  // 需要 panel target 才能启用的工具栏按钮 (close 永远可用)
  const TARGET_ACTIONS = { full: true, new: true, download: true, native: true };

  let selectedIndex = -1; // 内存光标: 当前视图内选中行下标 (identity 镜像进 state)

  function entryHost() {
    return state.view === 'grid' ? $('#cb-view-grid') : $('#cb-view-list');
  }

  function visibleEntries() {
    const host = entryHost();
    return host ? $$(ENTRY_SELECTOR, host) : [];
  }

  // 行/面板唯一 action target: 属性缺失或非法 → null (禁用全部 action)。
  function encodedPathOf(el) {
    if (!el || !el.dataset) return null;
    const value = el.dataset.nativePathEncoded;
    if (value === undefined || !isValidEncodedPath(value)) return null;
    return value;
  }

  function entryKindOf(el) {
    return el && el.dataset ? el.dataset.kind || null : null;
  }

  function findEntryByIdentity(identity) {
    if (identity === null) return null;
    const found = visibleEntries().find(function (el) {
      return encodedPathOf(el) === identity;
    });
    return found || null;
  }

  function removeEntrySelection() {
    visibleEntries().forEach(function (el) {
      el.classList.remove('cb-selected');
    });
  }

  // 该行在新 hidden 规则下是否仍可见: 无 identity 的显示行不可判定 → 视为可见
  // (非 UTF-8 display-only 行由服务端过滤规则决定).
  function entryHiddenByRules(el) {
    const identity = encodedPathOf(el);
    if (identity === null || identity === '') return false;
    return isHiddenEncodedPath(identity);
  }

  // Q17 隐藏焦点回退候选: 从当前光标起最近可见条目 (优先向后); 无选中 → null.
  function fallbackSelectionForHide() {
    if (selectedIndex < 0) return null;
    const entries = visibleEntries();
    const visible = entries.map(function (el) {
      return !entryHiddenByRules(el);
    });
    const idx = nearestVisibleIndex(visible, selectedIndex);
    return idx < 0 ? null : encodedPathOf(entries[idx]);
  }

  function clearEntrySelection(opts) {
    removeEntrySelection();
    selectedIndex = -1;
    state.selected = null;
    if (!(opts && opts.silent)) replaceCurrentEntry();
  }

  function selectIndex(index, opts) {
    const entries = visibleEntries();
    if (entries.length === 0) {
      clearEntrySelection(opts);
      return;
    }
    const clamped = Math.max(0, Math.min(index, entries.length - 1));
    removeEntrySelection();
    selectedIndex = clamped;
    const entry = entries[clamped];
    entry.classList.add('cb-selected');
    entry.scrollIntoView({ block: 'nearest' });
    state.selected = encodedPathOf(entry);
    // 选中非 actionable 行立即清空 panel target (绝不复用 stale target);
    // 打开条目的 identity 不受选中影响.
    panelTarget = panelTargetForSelection(state.selected, panelTarget);
    if (panelTarget === null) setActionsEnabled(false);
    if (!(opts && opts.silent)) replaceCurrentEntry();
  }

  function selectEntry(entry) {
    const idx = visibleEntries().indexOf(entry);
    if (idx >= 0) selectIndex(idx);
  }

  // 按编码 identity 恢复选中 (视图切换/内容重取后; 不猜 index).
  function selectByIdentity(identity, opts) {
    if (identity === null || identity === undefined) {
      clearEntrySelection(opts);
      return;
    }
    const idx = visibleEntries().findIndex(function (el) {
      return encodedPathOf(el) === identity;
    });
    if (idx >= 0) {
      selectIndex(idx, opts);
    } else {
      clearEntrySelection(opts);
    }
  }

  // 有界移动: 到边界停, 不循环。
  function moveSelection(delta) {
    const entries = visibleEntries();
    if (entries.length === 0) return;
    const base = selectedIndex < 0 ? (delta > 0 ? -1 : 0) : selectedIndex;
    selectIndex(base + delta);
  }

  function selectedTarget() {
    const entries = visibleEntries();
    if (selectedIndex < 0 || selectedIndex >= entries.length) return null;
    return encodedPathOf(entries[selectedIndex]);
  }

  // 选中条目的 data-kind (dir|file): PDF 判定需要 kind + 后缀双条件 —
  // 目录名 `folder.pdf` 必须按目录处理.
  function selectedEntryKind() {
    const entries = visibleEntries();
    if (selectedIndex < 0 || selectedIndex >= entries.length) return null;
    return entryKindOf(entries[selectedIndex]);
  }

  // 视图 DOM: list/grid 容器互斥显示 (阅读页无容器); 树壳由外壳逻辑独占.
  function applyViewContainers() {
    const list = $('#cb-view-list');
    const grid = $('#cb-view-grid');
    if (state.view === 'list') {
      if (list) list.hidden = false;
      if (grid) grid.hidden = true;
    } else if (state.view === 'grid') {
      if (list) list.hidden = true;
      if (grid) grid.hidden = false;
    }
  }

  // 重取/视图切换后重标选中行 (identity 驱动, 不用 selectedIndex 反推).
  function applyEntrySelection(opts) {
    if (state.selected === null) {
      clearEntrySelection(opts);
      return;
    }
    selectByIdentity(state.selected, opts);
  }

  /* ---------- 视图/规则状态转移 ---------- */

  // 进入 tree 时的内容区目标: list/grid 的已打开条目优先, 否则当前上下文.
  // opened 的语义 kind 优先取内存值, 缺失时从列表行 data-kind 推导 (没有竞态).
  function treeEntryTarget() {
    // 搜索切换只改变布局, 已打开预览不能把查询变成文件导航.
    if (state.kind === 'search') return { kind: 'search', path: '' };
    if (state.view === 'list' || state.view === 'grid') {
      const opened = state.opened;
      if (opened !== null && opened !== '' && isValidEncodedPath(opened)) {
        const entry = findEntryByIdentity(opened);
        const kind =
          state.openedKind ||
          (entry ? (entryKindOf(entry) === 'dir' ? 'directory' : 'reading') : null);
        if (kind) return { kind: kind, path: opened };
      }
    }
    return { kind: state.kind, path: state.path };
  }

  // tree → list/grid: 阅读目标进父目录并定位 (实际导航, 新增一个目标项);
  // 目录/搜索保持位置 (同位置 replace 后整页重取, 恢复本机打开能力).
  function listGridTargetFromTree() {
    if (state.kind === 'reading') {
      const parent = parentEncodedPath(state.path);
      const dir = parent === null ? '' : parent;
      return {
        kind: 'directory',
        path: dir,
        locate: state.path,
        opened: state.path,
        openedKind: 'reading',
        sameLocation: false,
      };
    }
    if (state.kind === 'search') {
      return {
        kind: 'search',
        path: '',
        locate: state.selected,
        opened: null,
        openedKind: null,
        sameLocation: isSearchLocation(),
      };
    }
    return {
      kind: 'directory',
      path: state.path,
      locate: null,
      opened: null,
      openedKind: null,
      sameLocation: locationEncodedPath() === state.path,
    };
  }

  // 用户视图切换 (控件或 v 键): 同位置变化 replace 当前项; 需要完整页能力时
  // 交接 (同路径 replace+reload, 跨路径只新增一个目标项+reload).
  function switchToView(next, opts) {
    if (next !== 'tree' && next !== 'list' && next !== 'grid') return;
    if (next === state.view) return;
    const user = !!(opts && opts.user);
    if (next === 'tree') {
      cancelListingRefresh();
      const target = treeEntryTarget();
      state.view = 'tree';
      state.kind = target.kind;
      state.path = target.path;
      state.q = target.kind === 'search' ? state.q : null;
      state.opened = null;
      state.openedKind = null;
      closePanel(); // 树壳没有预览面板: 关闭并作废在途预览
      if (user) persistView('tree');
      syncControlState();
      replaceCurrentEntry(
        isSearchLocation()
          ? currentEntryUrlWithRules()
          : contextUrl(state.kind, state.path, state.q, location.hash)
      );
      ensureTreeShell(true);
      return;
    }
    if (user) persistView(next);
    if (state.view === 'tree') {
      handoff(listGridTargetFromTree(), next);
      return;
    }
    state.view = next;
    syncControlState();
    applyViewContainers();
    applyEntrySelection();
    replaceCurrentEntry();
  }

  // 隐藏/排序控件变化: 立即提交新规则、当前历史项与用户改动的偏好字段.
  function applyRulesChange(next) {
    const change = {};
    if (next.sort !== undefined && next.sort !== null && next.sort !== state.rules.sort) {
      change.sort = next.sort;
    }
    if (next.hidden !== undefined && next.hidden !== null && next.hidden !== state.rules.hidden) {
      change.hidden = next.hidden;
    }
    if (change.sort === undefined && change.hidden === undefined) return;
    if (change.hidden === false) {
      // Q17 焦点回退: 只重标选中/焦点, 不打开回退项, 不改变 pathname/q/hash.
      state.selected =
        state.view === 'tree'
          ? treeHiddenFallback()
          : selectedIndex < 0
            ? null
            : fallbackSelectionForHide();
    }
    if (change.sort !== undefined) {
      state.rules.sort = change.sort;
      persistSort(change.sort);
    }
    if (change.hidden !== undefined) {
      state.rules.hidden = change.hidden;
      persistHidden(change.hidden);
    }
    optionsVersion += 1;
    syncControlState();
    replaceCurrentEntry(currentEntryUrlWithRules());
    if (state.view === 'tree') {
      // 树/内容区客户端重取: 已展开分支与目录预览进入新版本加载状态; 打开内容
      // 保留 (文档正文不因规则变化重转换), 重排不收起、不回根目录.
      refreshTreeForRules();
      refreshPaneForRules();
      return;
    }
    // 只安装服务器生成的条目区, 保留已打开文档的 live DOM 与本机打开能力.
    refreshListingForRules();
    if (state.opened !== null && state.openedKind === 'directory') {
      openPreview(state.opened, 'dir', { silent: true });
    }
  }

  let listingAbort = null;

  function cancelListingRefresh() {
    if (listingAbort) listingAbort.abort();
    listingAbort = null;
    const status = $('[data-cb-listing-status]');
    if (status) status.remove();
    const main = $('main.cb-main');
    if (main) {
      main.hidden = false;
      main.removeAttribute('aria-busy');
    }
  }

  // 完整可信页只取 main/面包屑, 不安装 head、token、工具区或预览面板.
  // 显示规则变化不重建文档正文; 新旧版本的成功/失败采用同一提交守卫.
  function refreshListingForRules() {
    cancelListingRefresh();
    const main = $('main.cb-main');
    if (!main) return;
    const version = optionsVersion;
    const kind = state.kind;
    const path = state.path;
    const query = state.q;
    const sort = state.rules.sort;
    const hidden = state.rules.hidden ? '1' : '0';
    const focusInMain = main.contains(document.activeElement);
    const current = function () {
      return (
        version === optionsVersion &&
        (state.view === 'list' || state.view === 'grid') &&
        kind === state.kind && path === state.path && query === state.q
      );
    };
    const status = document.createElement('p');
    status.setAttribute('data-cb-listing-status', '');
    status.setAttribute('role', 'status');
    status.textContent = '正在更新条目…';
    main.before(status);
    main.hidden = true;
    main.setAttribute('aria-busy', 'true');
    const ac = new AbortController();
    listingAbort = ac;
    fetch(contextUrl(kind, path, query, location.hash), {
      headers: { Accept: 'text/html' },
      signal: ac.signal,
    })
      .then(function (res) {
        if (!current()) return null;
        if (!res.ok) throw new Error('HTTP ' + res.status);
        if (
          !(res.headers.get('Content-Type') || '').toLowerCase().startsWith('text/html') ||
          res.headers.get('X-Frame-Options') !== 'DENY'
        ) throw new Error('无效的条目响应');
        return res.text();
      })
      .then(function (html) {
        if (html === null || !current()) return;
        const doc = new DOMParser().parseFromString(html, 'text/html');
        const data = doc.body.dataset;
        const nextMain = doc.querySelector('main.cb-main');
        if (
          !nextMain || data.cbSurface !== kind || data.nativePathEncoded !== path ||
          data.cbSort !== sort || data.cbHidden !== hidden ||
          (kind === 'search' && data.cbQuery !== query)
        ) throw new Error('条目响应与当前上下文不一致');
        main.replaceWith(nextMain);
        const breadcrumb = $('.cb-breadcrumb');
        const nextBreadcrumb = doc.querySelector('.cb-breadcrumb');
        if (breadcrumb && nextBreadcrumb) breadcrumb.replaceWith(nextBreadcrumb);
        document.body.dataset.cbSort = sort;
        document.body.dataset.cbHidden = hidden;
        status.remove();
        listingAbort = null;
        applyViewContainers();
        applyEntrySelection({ silent: true });
        if (focusInMain) {
          const selected = findEntryByIdentity(state.selected);
          nextMain.tabIndex = -1;
          (selected || nextMain).focus();
        }
        replaceCurrentEntry();
      })
      .catch(function (err) {
        if (!current() || (err && err.name === 'AbortError')) return;
        const error = document.createElement('p');
        error.setAttribute('role', 'alert');
        error.textContent = '无法更新条目：' + (err && err.message ? err.message : String(err));
        main.replaceChildren(error);
        main.hidden = false;
        main.removeAttribute('aria-busy');
        status.remove();
        listingAbort = null;
      });
  }

  /* ---------- 预览面板 (list/grid) ---------- */

  let panelTarget = null; // 合法 encodedPath 或 null (fail-closed)
  let panelCommittedKey = null; // 最近成功安装的内容键 (含上下文与规则)
  const previewGuard = createGenerationGuard();

  function panel() {
    return $(PANEL_SELECTOR);
  }

  function panelContent() {
    return $('#cb-preview-content');
  }

  function panelTitle() {
    return $('#cb-preview-title');
  }

  // 内容键: 目录/搜索内容依赖上下文、查询与显示规则; 文档正文与规则无关
  // (绝不因排序/隐藏重转换已打开文档).
  function contentKey(kind, encodedPath, query) {
    const parts = [kind, encodedPath, query || ''];
    if (kind === 'directory' || kind === 'search') {
      parts.push(state.rules.sort);
      parts.push(state.rules.hidden ? '1' : '0');
    }
    return parts.join('|');
  }

  function setActionsEnabled(enabled) {
    $$(PANEL_SELECTOR + ' ' + ACTION_SELECTOR).forEach(function (btn) {
      if (TARGET_ACTIONS[btn.getAttribute('data-cb-action')]) {
        btn.disabled = !enabled;
      }
    });
  }

  // 下载按钮单独禁用: 目录 target 的 download 被服务端忽略 (返回目录页),
  // 其余 target 相关 action (full/new/native) 对目录仍有效.
  function setDownloadEnabled(enabled) {
    const btn = $(PANEL_SELECTOR + ' [data-cb-action="download"]');
    if (btn) btn.disabled = !enabled;
  }

  function clearPanelTarget() {
    panelTarget = null;
    setActionsEnabled(false);
  }

  function installPanelTarget(encodedPath) {
    if (!isValidEncodedPath(encodedPath) || encodedPath === '') {
      clearPanelTarget();
      return;
    }
    panelTarget = encodedPath;
    setActionsEnabled(true);
  }

  // 预览标题纯文本 (错误/清空): textContent, 绝不注入 HTML.
  function setPanelTitleText(text) {
    const el = panelTitle();
    if (!el) return;
    const bdi = el.querySelector('bdi');
    if (bdi) {
      bdi.textContent = text;
    } else {
      el.textContent = text;
    }
  }

  // 预览标题 markup (fragment 携带的逐段 bdi 安全 label): 移动节点而非
  // 字符串注入; 来源是服务端 maud 生成且经显示 codec 转义的 markup.
  function installPanelTitleMarkup(titleEl) {
    const el = panelTitle();
    if (!el) return;
    el.replaceChildren();
    if (titleEl) {
      el.append(...titleEl.childNodes);
    }
  }

  function openPanel() {
    const el = panel();
    if (!el) return;
    el.hidden = false;
    document.body.classList.add('cb-panel-open');
  }

  function closePanel() {
    previewGuard.invalidate(); // 迟到的预览响应/错误不得安装状态
    const el = panel();
    if (el) el.hidden = true;
    document.body.classList.remove('cb-panel-open');
    clearPanelTarget();
    panelCommittedKey = null;
    setPanelTitleText('');
    const host = panelContent();
    if (host) {
      host.textContent = '';
      host.removeAttribute('data-cb-content-state');
    }
    if (state.opened !== null || state.openedKind !== null) {
      state.opened = null;
      state.openedKind = null;
      replaceCurrentEntry();
    }
  }

  function showPanelError(message) {
    const host = panelContent();
    if (!host) return;
    const priorError = host.querySelector('[data-cb-preview-error]');
    if (priorError) priorError.remove();
    const error = document.createElement('p');
    error.setAttribute('data-cb-preview-error', '');
    error.setAttribute('role', 'alert');
    error.textContent = message;
    host.append(error);
    // 导航失败不破坏先前安装的正文与 identity; 没有旧目标时仍禁用 actions.
    if (state.opened !== null && isValidEncodedPath(state.opened)) {
      installPanelTarget(state.opened);
      setDownloadEnabled(state.openedKind !== 'directory');
    }
  }

  function flashNativeFeedback(text) {
    const el = $('.cb-native-feedback');
    if (!el) return;
    el.textContent = text;
    clearTimeout(flashNativeFeedback._timer);
    flashNativeFeedback._timer = setTimeout(function () {
      el.textContent = '';
    }, 1600);
  }

  // fragment wrapper 恒为响应的根元素; 面板/内容区 identity 只从根 wrapper 自身
  // 的 data 读取, 绝不从后代元素 (迷你列表条目等) 反推.
  function findFragmentWrapper(host) {
    return host.firstElementChild;
  }

  function wrapperSurface(wrapper) {
    const value = wrapper && wrapper.dataset ? wrapper.dataset.cbSurface : null;
    if (value === 'directory' || value === 'search' || value === 'reading') return value;
    return null;
  }

  // 非成功内容 (占位/忙/错误 body) 绝不是可跳过重取的成功安装: 标记可能落在
  // wrapper 自身 (fragment 错误根) 或 wrapper 内的 no_preview 根 (文件包装).
  function wrapperIsPlaceholder(wrapper) {
    if (!wrapper || !wrapper.matches) return false;
    return (
      wrapper.matches('[data-cb-content-state="placeholder"]') ||
      wrapper.querySelector('[data-cb-content-state="placeholder"]') !== null
    );
  }

  // 安装 side metadata: 内容状态镜像到宿主 (成功内容移除 placeholder 标记).
  function mirrorContentState(host, wrapper) {
    if (!host) return;
    if (wrapperIsPlaceholder(wrapper)) {
      host.setAttribute('data-cb-content-state', 'placeholder');
    } else {
      host.removeAttribute('data-cb-content-state');
    }
  }

  // 预览: 先清空旧 target/actions 再 fetch; 要求 ok + text/html 才 innerHTML,
  // 错误用 textContent; target 只从返回 wrapper 的合法 encoded data 安装。
  // generation 守卫: 新请求/面板关闭使在途请求过期, 旧响应/错误不得安装状态。
  function openPreview(encodedPath, kind, opts) {
    if (!isValidEncodedPath(encodedPath) || encodedPath === '') return; // fail-closed
    const silent = !!(opts && opts.silent);
    // PDF: 面板承载不了原生查看器 — 用户操作时新 tab 打开并返回; 恢复不触发.
    if (isPdfFile(encodedPath, kind)) {
      if (!silent) {
        window.open(browseActionUrl(encodedPath, 'full').href, '_blank', 'noopener');
      }
      return;
    }
    // 预览内容键: 目录预览随显示规则失效, 文档正文与规则无关 (绝不重转换).
    const requestedKind = kind === 'file' ? 'reading' : 'directory';
    const key = contentKey(requestedKind, encodedPath, null);
    const sort = state.rules.sort;
    const hidden = state.rules.hidden ? '1' : '0';
    const gen = previewGuard.begin(); // 新请求作废所有在途请求
    clearPanelTarget();
    openPanel();
    const priorError = panelContent() && panelContent().querySelector('[data-cb-preview-error]');
    if (priorError) priorError.remove();
    if (panelCommittedKey === key && !(opts && opts.force)) {
      // 内容未变 (同 identity 同规则): 不重取, 仅恢复 target/打开条目.
      installPanelTarget(encodedPath);
      state.opened = encodedPath;
      replaceCurrentEntry();
      return;
    }
    const url = browseActionUrl(encodedPath, 'fragment');
    fetch(url, { headers: { Accept: 'text/html' } })
      .then(function (res) {
        if (!previewGuard.isCurrent(gen)) return null; // 过期响应: 丢弃
        return readFragmentResponse(res);
      })
      .then(function (html) {
        if (html === null || !previewGuard.isCurrent(gen)) return; // 过期响应: 丢弃
        const host = panelContent();
        if (!host) return;
        const parsed = document.createElement('div');
        parsed.innerHTML = html;
        const wrapper = findFragmentWrapper(parsed);
        if (!wrapper) throw new Error('无效的预览响应');
        const identity =
          typeof wrapper.dataset.nativePathEncoded === 'string' &&
          isValidEncodedPath(wrapper.dataset.nativePathEncoded) &&
          wrapper.dataset.nativePathEncoded !== ''
            ? wrapper.dataset.nativePathEncoded
            : null;
        const surface = wrapperSurface(wrapper);
        if (
          identity !== encodedPath || surface !== requestedKind ||
          (surface === 'directory' &&
            (wrapper.dataset.cbSort !== sort || wrapper.dataset.cbHidden !== hidden))
        ) throw new Error('预览响应与请求上下文不一致');
        host.replaceChildren(wrapper);
        installPanelTarget(identity);
        mirrorContentState(host, wrapper);
        // 打开条目 identity: 只接受与服务端 wrapper 一致的合法目标.
        state.opened = identity;
        state.openedKind = surface === 'directory' ? 'directory' : surface === 'reading' ? 'reading' : null;
        replaceCurrentEntry();
        if (panelTarget !== null) {
          setDownloadEnabled(surface !== 'directory');
        }
        installPanelTitleMarkup(wrapper.querySelector('.cb-frag-title'));
        renderMermaidIn(host); // 预览面板 fragment 内的 mermaid 容器
        // 只有一致且非占位内容才记为可跳过重取的成功安装.
        if (!wrapperIsPlaceholder(wrapper)) {
          panelCommittedKey = key;
        } else {
          panelCommittedKey = null;
        }
      })
      .catch(function (err) {
        if (!previewGuard.isCurrent(gen)) return; // 过期错误: 丢弃
        clearPanelTarget();
        showPanelError('无法预览：' + (err && err.message ? err.message : String(err)));
      });
  }

  // native-open: 单次 decode + URLSearchParams form; 204 给短暂反馈。
  function openNative(encodedPath) {
    if (nativeToken === null) return;
    if (!isValidEncodedPath(encodedPath)) return;
    const params = nativeOpenParams(encodedPath, nativeToken);
    if (params === null) {
      clearPanelTarget(); // decode 失败: 清状态, 不发送请求
      return;
    }
    fetch('/__/native-open', {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      body: params.toString(),
    })
      .then(function (res) {
        if (res.status === 204) {
          flashNativeFeedback('已用本机应用打开');
        } else {
          flashNativeFeedback('打开失败（HTTP ' + res.status + '）');
        }
      })
      .catch(function () {
        flashNativeFeedback('打开失败');
      });
  }

  function handleAction(event, btn) {
    const action = btn.getAttribute('data-cb-action');
    // 树视图无 native-open (Q17 能力边界): native action 一律 no-op
    if (action === 'native' && state.view === 'tree') return;
    if (action === 'close') {
      event.preventDefault();
      closePanel();
      return;
    }
    const entry = btn.closest(ENTRY_SELECTOR);
    const target = entry
      ? encodedPathOf(entry)
      : btn.closest(PANEL_SELECTOR)
        ? panelTarget
        : null;
    if (target === null) return; // 非 actionable: no-op
    event.preventDefault();
    if (action === 'preview') {
      openPreview(target, entry ? entryKindOf(entry) : null);
    } else if (action === 'full') {
      location.href = browseActionUrl(target, 'full').href;
    } else if (action === 'new') {
      window.open(browseActionUrl(target, 'full').href, '_blank', 'noopener');
    } else if (action === 'download') {
      location.href = browseActionUrl(target, 'download').href;
    } else if (action === 'native') {
      openNative(target);
    }
  }

  function togglePreview() {
    const target = selectedTarget();
    if (target === null) return;
    const el = panel();
    if (el && !el.hidden && panelTarget === target) {
      closePanel();
      return;
    }
    openPreview(target, selectedEntryKind());
  }

  /* ---------- 树壳 (tree 视图; 懒加载 + 显示规则版本化) ---------- */

  const TREE_EXPANDED_KEY = 'cb-tree-expanded';
  const TREE_SELECTED_KEY = 'cb-tree-selected';
  const TREE_WIDTH_KEY = 'cb-tree-width'; // 树宽持久化键 (px)
  const TREE_COLLAPSED_KEY = 'cb-tree-collapsed';
  const TREE_DEFAULT_WIDTH = 238;
  const TREE_MIN_WIDTH = 160;
  const TREE_MAX_WIDTH = 480;
  const TREE_DRAWER_MQL = '(width < 64rem)'; // 窄屏抽屉断点 (与 CSS 媒体查询同源)

  let treeShell = null; // nav.cb-tree 外壳
  let treeRootUl = null; // 根 <ul>
  let treeResizer = null; // 列 2 调整器元素 (随树壳创建)
  let treeCollapseBtn = null; // 折叠按钮 (树壳顶栏子控件)
  let treeScrim = null; // 遮罩: 点击收起抽屉
  let treeOpenBtn = null; // 唤起按钮 (仅窄屏可见)
  let treeRestoreBtn = null; // 左缘恢复按钮 (仅折叠生效时可见)
  let treeAbortControllers = new Set(); // 本壳全部在途树请求 (退出/版本变化时中止)
  let treeGen = 0; // 树壳生命周期 generation: 退出/重建递增使全部在途响应失效
  let treeSelectedEl = null;
  let treeItemSeq = 0; // 稳定节点 id 计数器 (壳内唯一; 每次建壳复位)
  let drawerMql = null;
  let drawerMqlHandler = null;
  let treeViewportResizeHandler = null;
  let treeCollapseMql = null;
  let treeCollapseMqlHandler = null;
  let treeFocusKind = null; // 跨断点焦点追踪 (见 applyTreeCollapsed)
  let treePreferredWidth = TREE_DEFAULT_WIDTH; // 持久化偏好: 仅硬边界截断
  let treeWidth = TREE_DEFAULT_WIDTH; // 当前生效宽
  let treeResizePointerId = null; // 在途拖拽手势 pointerId (单 owner)
  let treeCollapsePreferred = false;
  let treeCollapsed = false;

  // 显示规则版本化的分支缓存/意图: 只有当前版本的结果可安装; 版本变化整体失效.
  const treeBranchCache = new Map(); // path -> { version, entries }
  const treeExpandIntent = new Map(); // path -> boolean (最新显式展开意图, 安装时消费)
  const treePending = new Map(); // path -> { version, promise, controller }
  // 树内容链 (展开/恢复) 代次: 新的导航或规则变化使旧链只共享缓存, 不提交 DOM.
  const treeContentGuard = createGenerationGuard();

  let treeExpandedPaths = loadJsonArray(TREE_EXPANDED_KEY, []);

  function loadJsonArray(key, fallback) {
    try {
      const value = JSON.parse(localStorage.getItem(key) || 'null');
      return Array.isArray(value) ? value : fallback;
    } catch (err) {
      return fallback;
    }
  }

  function saveJsonArray(key, value) {
    try {
      localStorage.setItem(key, JSON.stringify(value));
    } catch (err) {
      /* 隐私模式等场景忽略持久化失败 */
    }
  }

  function treeUrl(path) {
    const base = path === '' ? '/__/tree' : '/__/tree/' + path;
    return (
      base + '?sort=' + state.rules.sort + '&hidden=' + (state.rules.hidden ? '1' : '0')
    );
  }

  // 拉取目录子项 JSON: path 为空串时命中显式根路由 /__/tree (绝不拼尾斜杠);
  // 请求带有效显示规则 (树不再忽略 sort/hidden).
  function fetchTreeEntries(path, signal) {
    return fetch(treeUrl(path), {
      headers: { Accept: 'application/json' },
      signal: signal,
    })
      .then(function (res) {
        if (!res.ok) throw new Error('HTTP ' + res.status);
        return res.json();
      })
      .then(function (data) {
        return data && Array.isArray(data.entries) ? data.entries : [];
      });
  }

  function treePathFromHref(href) {
    // '/a%20b/c.md' -> 'a%20b/c.md'; 必须过合法 encodedPath 校验 (fail-closed)
    if (typeof href !== 'string' || href[0] !== '/') return null;
    const rel = href.slice(1);
    return isValidEncodedPath(rel) ? rel : null;
  }

  // JSON -> DOM 节点: 文件名只经 createElement + textContent (外层 bdi),
  // 绝不插值进 innerHTML/HTML 模板. href 过合法 encodedPath 校验后进 dataset.
  function buildTreeNode(entry) {
    const li = document.createElement('li');
    li.className = 'cb-tree-node';
    const name = typeof entry.name === 'string' ? entry.name : '';
    const isDir = entry.dir === true;
    li.id = 'cb-tree-item-' + ++treeItemSeq; // 稳定 id: ARIA 活动后代公告锚点
    li.setAttribute('role', 'treeitem');
    li.setAttribute('aria-selected', 'false');
    const href = typeof entry.href === 'string' ? entry.href : null;
    const relPath = treePathFromHref(href);
    // 可展开 = 目录 && 服务端 expandable && 有合法 href (环/非 UTF-8 目录恒不可展开)
    const expandable = isDir && entry.expandable === true && relPath !== null;
    // aria-expanded 只挂可展开目录: 终端目录没有子项, 不得向 AT 伪装可展开
    if (expandable) li.setAttribute('aria-expanded', 'false');
    li.dataset.treeDir = isDir ? '1' : '0';
    li.dataset.treeExpandable = expandable ? '1' : '0';
    if (relPath !== null) {
      li.dataset.nativePathEncoded = relPath;
    }

    const row = document.createElement('div');
    row.className = 'cb-tree-row';

    const twist = document.createElement('span');
    twist.className = 'cb-tree-twist';
    if (expandable) twist.textContent = '▸';
    twist.setAttribute('aria-hidden', 'true'); // 装饰字形: 可访问名只来自文件名 bdi
    row.append(twist);

    const icon = document.createElement('span');
    icon.className = 'cb-tree-icon';
    icon.textContent = isDir ? '📁' : '📄';
    icon.setAttribute('aria-hidden', 'true'); // 装饰字形: 可访问名只来自文件名 bdi
    row.append(icon);

    if (!isDir) {
      const badge = document.createElement('span');
      badge.className = 'cb-tree-badge';
      const dot = name.lastIndexOf('.');
      const ext = dot > 0 ? name.slice(dot + 1) : '';
      badge.textContent = ext ? ext.slice(0, 5).toUpperCase() : 'FILE';
      badge.setAttribute('aria-hidden', 'true'); // 扩展名徽标是装饰: 不进入可访问名
      row.append(badge);
    }

    const bdi = document.createElement('bdi');
    bdi.dir = 'auto';
    bdi.textContent = name;
    row.append(bdi);

    li.append(row);
    return li;
  }

  function childrenUlFor(li) {
    let ul = li.querySelector(':scope > ul.cb-tree-children');
    if (!ul) {
      ul = document.createElement('ul');
      ul.className = 'cb-tree-children';
      ul.setAttribute('role', 'group');
      li.append(ul);
    }
    return ul;
  }

  function isDirNode(li) {
    return li.dataset.treeDir === '1';
  }

  function isExpandableDir(li) {
    return li.dataset.treeDir === '1' && li.dataset.treeExpandable === '1';
  }

  function nodeHref(li) {
    const path = encodedPathOf(li);
    return path === null || path === '' ? null : browseActionUrl(path, 'full');
  }

  function setTwist(li, expanded) {
    const twist = li.querySelector(':scope > .cb-tree-row .cb-tree-twist');
    if (twist) twist.textContent = expanded ? '▾' : '▸';
  }

  // 分支状态标记 (loading/error/empty; 缺席 = 已加载).
  function setBranchState(li, status) {
    if (status === null || status === undefined) {
      li.removeAttribute('data-cb-branch-state');
      li.removeAttribute('aria-busy');
      return;
    }
    li.setAttribute('data-cb-branch-state', status);
    if (status === 'loading') li.setAttribute('aria-busy', 'true');
    else li.removeAttribute('aria-busy');
  }

  function isBranchInstalled(li) {
    const branchState = li.getAttribute('data-cb-branch-state');
    if (branchState === 'empty' || branchState === 'error') return true;
    if (branchState === 'loading') return false;
    const ul = li.querySelector(':scope > ul.cb-tree-children');
    return !!(ul && ul.childElementCount > 0);
  }

  // 展开/收起 (persist: 只在用户动作与导航默认展开时写偏好; 版本重取不写).
  function setTreeNodeExpanded(li, expanded, persist) {
    const ul = li.querySelector(':scope > ul.cb-tree-children');
    if (ul) ul.hidden = !expanded;
    if (isExpandableDir(li)) {
      setTwist(li, expanded);
      li.setAttribute('aria-expanded', expanded ? 'true' : 'false');
    }
    const path = encodedPathOf(li);
    if (path === null || path === '') return;
    const idx = treeExpandedPaths.indexOf(path);
    if (expanded && idx < 0) {
      treeExpandedPaths.push(path);
    } else if (!expanded && idx >= 0) {
      treeExpandedPaths.splice(idx, 1);
    } else if (!persist) {
      return;
    }
    if (persist) saveJsonArray(TREE_EXPANDED_KEY, treeExpandedPaths);
  }

  // 只在被替换的子树内清除选中 (节点自身不因安装其子项而失去选中/identity).
  function clearSelectionWithin(li) {
    if (treeSelectedEl && treeSelectedEl !== li && li.contains(treeSelectedEl)) {
      clearTreeSelection();
    }
  }

  // 分支成功内容安装 (唯一 replaceChildren 点): 空结果只表示当前显示规则版本
  // 没有可见孩子 — 不叶子化、不删除展开意图; 服务器明确不可展开的目录保持.
  function installTreeBranch(li, path, entries, version, ownership) {
    const decision = treeLoadDecision(
      true,
      version === optionsVersion,
      !!(ownership && ownership.required),
      !ownership || !ownership.required ? false : ownership.current()
    );
    if (decision !== 'install') return false;
    clearSelectionWithin(li);
    if (entries.length === 0) {
      childrenUlFor(li).replaceChildren();
      setBranchState(li, 'empty');
    } else {
      const ul = childrenUlFor(li);
      ul.replaceChildren(...entries.map(buildTreeNode));
      setBranchState(li, null);
    }
    applyTreeBranchIntent(li, path);
    return true;
  }

  // 终态跟随最新显式展开意图 (在途期间用户可再次激活表达收起); 记录被消费后
  // 删除, DOM 即权威.
  function applyTreeBranchIntent(li, path) {
    const desired = treeExpandIntent.get(path);
    treeExpandIntent.delete(path);
    setTreeNodeExpanded(li, desired === undefined ? true : desired, false);
  }

  // 当前版本分支失败: 安全错误状态 — 不留旧集合、不视为空目录、不删展开意图.
  function installTreeBranchError(li, path, version) {
    if (version !== optionsVersion || !li.isConnected) return false;
    clearSelectionWithin(li);
    const ul = childrenUlFor(li);
    ul.replaceChildren();
    const item = document.createElement('li');
    item.className = 'cb-tree-branch-state';
    item.setAttribute('role', 'none');
    const row = document.createElement('div');
    row.className = 'cb-tree-branch-error';
    row.setAttribute('role', 'alert');
    row.textContent = '无法加载目录分支';
    item.append(row);
    ul.append(item);
    setBranchState(li, 'error');
    applyTreeBranchIntent(li, path);
    return true;
  }

  function installTreeBranchLoading(li) {
    clearSelectionWithin(li);
    const ul = childrenUlFor(li);
    ul.replaceChildren();
    const item = document.createElement('li');
    item.className = 'cb-tree-branch-state';
    item.setAttribute('role', 'none');
    const row = document.createElement('span');
    row.className = 'cb-tree-branch-loading';
    row.setAttribute('aria-hidden', 'true');
    row.textContent = '加载中…';
    item.append(row);
    ul.append(item);
    setBranchState(li, 'loading');
    // 加载中即展开态 (可见加载行与 aria-expanded 一致); 终态由安装方按最新
    // 意图落地.
    setTreeNodeExpanded(li, true, false);
  }


  // 懒加载目录子项 (显示规则版本化):
  // - 同版本在途请求复用同一 Promise (绝不二次 fetch); 旧版本在途一律丢弃且
  //   在版本变化时整体中止;
  // - 成功: 当前版本写入缓存并按所有权安装 (陈旧导航只共享缓存, 不提交 DOM);
  // - 失败: 仅当前版本显示安全错误; 旧版本错误丢弃; 不加自动重试.
  function loadTreeBranch(li, path, ownership) {
    const shellGen = treeGen;
    const version = optionsVersion;
    const required = !!(ownership && ownership.required);
    const currentFn = required ? ownership.current : null;
    if (required && !currentFn()) return Promise.resolve(false);
    // 当前导航先发布默认展开意图; 之后的显式点击仍可覆盖它.
    if (required) treeExpandIntent.set(path, true);
    const cached = treeBranchCache.get(path);
    if (cached && cached.version === version) {
      if (!isBranchInstalled(li)) {
        return Promise.resolve(installTreeBranch(li, path, cached.entries, version, ownership));
      }
      applyTreeBranchIntent(li, path);
      return Promise.resolve(true);
    }
    if (cached) treeBranchCache.delete(path);
    let pending = treePending.get(path);
    if (pending && pending.version !== version) {
      treePending.delete(path);
      try {
        pending.controller.abort();
      } catch (err) {
        /* 已结束: 忽略 */
      }
      pending = null;
    }
    if (!pending) {
      const controller = new AbortController();
      treeAbortControllers.add(controller);
      // 共享传输只缓存结果, 不持有发起导航的 DOM 提交权.
      const request = { version: version, controller: controller, promise: null };
      request.promise = fetchTreeEntries(path, controller.signal)
        .then(function (entries) {
          if (shellGen === treeGen && version === optionsVersion) {
            treeBranchCache.set(path, { version: version, entries: entries });
          }
          return entries;
        })
        .finally(function () {
          treeAbortControllers.delete(controller);
          // 旧版本结束不得删除同路径的新请求身份.
          if (treePending.get(path) === request) treePending.delete(path);
        });
      treePending.set(path, request);
      pending = request;
      if (li.isConnected) installTreeBranchLoading(li);
    }
    // 每个续接方独立检查所有权: 陈旧导航不能阻止最新点击消费同一结果/错误.
    return pending.promise.then(
      function (entries) {
        const decision = treeLoadDecision(
          shellGen === treeGen,
          version === optionsVersion,
          required,
          required ? currentFn() : false
        );
        if (decision !== 'install' || !li.isConnected) return false;
        if (isBranchInstalled(li)) return true;
        return installTreeBranch(li, path, entries, version, ownership);
      },
      function (err) {
        if (err && err.name === 'AbortError') return false;
        const decision = treeLoadDecision(
          shellGen === treeGen,
          version === optionsVersion,
          required,
          required ? currentFn() : false
        );
        if (decision !== 'install' || !li.isConnected) return false;
        if (!isBranchInstalled(li)) installTreeBranchError(li, path, version);
        return false;
      }
    );
  }

  function abortTreeRequests() {
    treeAbortControllers.forEach(function (controller) {
      try {
        controller.abort();
      } catch (err) {
        /* 已结束: 忽略 */
      }
    });
    treeAbortControllers = new Set();
    treePending.clear();
    treeExpandIntent.clear();
  }

  // 显示规则变化: 版本递增后所有旧缓存/旧在途/旧错误一律失效, 重新取根并恢复
  // 展开与选中 (不收起、不回根目录、不抢焦点); 内容区目录/搜索预览同步重取.
  function refreshTreeForRules() {
    if (!treeShell || !treeRootUl) return;
    const expanded = expandedTreePaths(); // 版本变化前已展开的分支: 重排不收起
    treeBranchCache.clear();
    treeContentGuard.invalidate();
    abortTreeRequests();
    loadTreeRoot({ focusInTree: treeHasFocus(), expandedPaths: expanded });
  }

  function treeHasFocus() {
    return !!(treeShell && document.activeElement && treeShell.contains(document.activeElement));
  }

  function removeTreeError() {
    if (!treeShell) return;
    const prev = treeShell.querySelector('.cb-tree-error');
    if (prev) prev.remove();
  }

  function showTreeError(message) {
    if (!treeShell || !treeRootUl) return;
    treeRootUl.replaceChildren();
    treeRootUl.removeAttribute('aria-busy');
    removeTreeError();
    const errEl = document.createElement('div');
    errEl.className = 'cb-tree-error';
    errEl.setAttribute('role', 'alert');
    errEl.textContent = message;
    treeShell.append(errEl);
  }

  // 根加载: 旧集合立即让位 (新规则下不可操作), 成功安装后恢复展开/选中.
  function loadTreeRoot(opts) {
    if (!treeRootUl) return;
    const shellGen = treeGen;
    const version = optionsVersion;
    const focusInTree = !!(opts && opts.focusInTree);
    removeTreeError();
    treeRootUl.replaceChildren();
    treeRootUl.setAttribute('aria-busy', 'true');
    const controller = new AbortController();
    treeAbortControllers.add(controller);
    const settle = function () {
      treeAbortControllers.delete(controller);
      if (shellGen === treeGen && version === optionsVersion && treeRootUl) {
        treeRootUl.removeAttribute('aria-busy');
      }
    };
    fetchTreeEntries('', controller.signal).then(
      function (entries) {
        settle();
        const decision = treeLoadDecision(shellGen === treeGen, version === optionsVersion, false, false);
        if (decision !== 'install') return;
        treeRootUl.replaceChildren(...entries.map(buildTreeNode));
        restoreTreeContent({
          focusInTree: focusInTree,
          expandedPaths: opts && Array.isArray(opts.expandedPaths) ? opts.expandedPaths : [],
          revealCurrent: !!(opts && opts.revealCurrent),
        });
      },
      function (err) {
        settle();
        if (err && err.name === 'AbortError') return;
        const decision = treeLoadDecision(shellGen === treeGen, version === optionsVersion, false, false);
        if (decision !== 'install') return;
        showTreeError('无法加载目录树');
      }
    );
  }

  // 当前内容区路径 (search 无文件系统路径 → null).
  function currentPathFromState() {
    if (state.kind === 'search') return null;
    return state.path;
  }

  // 在当前层查找 href 匹配的树节点 (pathname 比较, 不 decode).
  function findTreeNode(parentUl, href) {
    return $$(':scope > li', parentUl).find(function (li) {
      const url = nodeHref(li);
      return url !== null && url.pathname === href;
    });
  }

  // 导航展开 encoded 前缀链; 恢复记录时只沿仍展开的祖先下钻, 不打开已折叠父项.
  // 返回最深节点. 链上所有分支加载都携带同一所有权 — 陈旧链只共享缓存.
  function walkTreePath(encodedPath, ownership, preserveCollapse) {
    const segments = encodedPath === '' ? [] : encodedPath.split('/');
    let parentUl = treeRootUl;
    let lastNode = null;
    let chain = Promise.resolve();
    segments.forEach(function (seg, i) {
      const href = '/' + segments.slice(0, i + 1).join('/');
      chain = chain.then(function () {
        if (!parentUl) return;
        if (ownership.required && !ownership.current()) {
          parentUl = null; // 陈旧链: 立即终止, 不再下钻
          return;
        }
        const node = findTreeNode(parentUl, href);
        if (!node) {
          parentUl = null;
          return;
        }
        lastNode = node;
        if (isExpandableDir(node)) {
          const path = encodedPathOf(node);
          if (path === null || (preserveCollapse && !treeExpandedPaths.includes(path))) {
            parentUl = null;
            return;
          }
          return loadTreeBranch(node, path, ownership).then(function () {
            if (ownership.required && !ownership.current()) {
              parentUl = null;
              return;
            }
            parentUl = node.querySelector(':scope > ul.cb-tree-children');
          });
        }
        parentUl = null;
      });
    });
    return chain.then(function () {
      return lastNode;
    });
  }

  function clearTreeCurrent() {
    if (!treeRootUl) return;
    $$('.cb-current', treeRootUl).forEach(function (el) {
      el.classList.remove('cb-current');
    });
  }

  // 同步标记当前 URL 路径链 (全部祖先 + 最深节点), 返回最深节点.
  function markTreePath(segments) {
    let parentUl = treeRootUl;
    let lastNode = null;
    segments.forEach(function (seg, i) {
      const href = '/' + segments.slice(0, i + 1).join('/');
      const node = parentUl ? findTreeNode(parentUl, href) : null;
      if (!node) {
        parentUl = null;
        return;
      }
      lastNode = node;
      node.classList.add('cb-current');
      const children = node.querySelector(':scope > ul.cb-tree-children');
      parentUl = isDirNode(node) && children && !children.hidden ? children : null;
    });
    return lastNode;
  }

  function visibleTreeNodes() {
    const result = [];
    function walk(ul) {
      $$(':scope > li', ul).forEach(function (li) {
        result.push(li);
        const children = li.querySelector(':scope > ul.cb-tree-children');
        if (children && !children.hidden) walk(children);
      });
    }
    if (treeRootUl) walk(treeRootUl);
    return result;
  }

  function selectTreeNode(li, opts) {
    if (treeSelectedEl) {
      treeSelectedEl.classList.remove('cb-selected');
      treeSelectedEl.setAttribute('aria-selected', 'false');
    }
    treeSelectedEl = li;
    li.classList.add('cb-selected');
    li.setAttribute('aria-selected', 'true');
    if (treeRootUl) treeRootUl.setAttribute('aria-activedescendant', li.id);
    li.scrollIntoView({ block: 'nearest' }); // 定位: 选中即保证可见
    const path = encodedPathOf(li);
    state.selected = path;
    if (!(opts && opts.silent)) {
      try {
        if (path === null) {
          localStorage.removeItem(TREE_SELECTED_KEY);
        } else {
          localStorage.setItem(TREE_SELECTED_KEY, path);
        }
      } catch (err) {
        /* 持久化失败忽略 */
      }
      replaceCurrentEntry();
    }
  }

  function clearTreeSelection(opts) {
    if (treeSelectedEl) {
      treeSelectedEl.classList.remove('cb-selected');
      treeSelectedEl.setAttribute('aria-selected', 'false');
    }
    treeSelectedEl = null;
    state.selected = null;
    if (treeRootUl) treeRootUl.removeAttribute('aria-activedescendant');
    if (!(opts && opts.silent)) replaceCurrentEntry();
  }

  // 恢复选中 identity: 快照/状态优先; 只有没有有效恢复快照的冷访问才回落到
  // 持久化 cb-tree-selected (不从共享选中偏好猜恢复对象); 不猜 index.
  function restoreTreeSelection(opts) {
    const snapshotTarget = state.selected;
    const node = snapshotTarget === null ? null : findVisibleTreeNode(snapshotTarget);
    if (node) {
      selectTreeNode(node, { silent: true });
      return;
    }
    if (snapshotTarget !== null || bootSnapshot) {
      clearTreeSelection({ silent: true });
      return;
    }
    let saved = null;
    try {
      saved = localStorage.getItem(TREE_SELECTED_KEY);
    } catch (err) {
      return;
    }
    if (saved === null || !isValidEncodedPath(saved)) return;
    const persisted = findVisibleTreeNode(saved);
    if (persisted) selectTreeNode(persisted, { silent: true });
  }

  function findVisibleTreeNode(identity) {
    return (
      visibleTreeNodes().find(function (li) {
        return encodedPathOf(li) === identity;
      }) || null
    );
  }

  // Q17 树焦点回退候选: 最近可见祖先 (自身仍可见则不变), 否则首个可见节点.
  function treeHiddenFallback() {
    if (!treeSelectedEl) return null;
    let node = treeSelectedEl;
    while (node) {
      const path = encodedPathOf(node);
      if (!(path !== null && path !== '' && isHiddenEncodedPath(path))) {
        return path;
      }
      const parentUl = node.parentElement;
      node = parentUl ? parentUl.closest('li.cb-tree-node') : null;
    }
    const first = visibleTreeNodes()[0];
    return first ? encodedPathOf(first) : null;
  }

  function moveTreeSelection(delta) {
    const nodes = visibleTreeNodes();
    if (nodes.length === 0) return;
    const idx = nodes.indexOf(treeSelectedEl);
    const base = idx < 0 ? (delta > 0 ? -1 : 0) : idx;
    const next = Math.max(0, Math.min(base + delta, nodes.length - 1));
    selectTreeNode(nodes[next]);
  }

  // 树根聚焦 (preventScroll): 指针选中树行后焦点落树根 — active-descendant 由根
  // 公告, 键盘导航立即接管; 老环境不支持 focus options 时退化为普通 focus.
  function focusTreeRootUl() {
    if (!treeRootUl) return;
    try {
      treeRootUl.focus({ preventScroll: true });
    } catch (err) {
      treeRootUl.focus();
    }
  }

  // 当前 DOM 中已展开的分支路径 (用于版本变化后的展开状态保持: 重排不收起).
  function expandedTreePaths() {
    const paths = [];
    if (!treeRootUl) return paths;
    $$('li.cb-tree-node', treeRootUl).forEach(function (li) {
      const ul = li.querySelector(':scope > ul.cb-tree-children');
      if (!ul || ul.hidden) return;
      const path = encodedPathOf(li);
      if (path !== null && path !== '') paths.push(path);
    });
    return paths;
  }

  // 恢复壳/刷新规则只恢复既有展开; 新导航入口才定位展开当前内容路径.
  // 标记 current 与恢复选中; 只有焦点原本位于树内时才回到树根 (绝不抢焦点).
  function restoreTreeContent(opts) {
    const shellGen = treeGen;
    const version = optionsVersion;
    const gen = treeContentGuard.begin();
    const ownership = {
      required: true,
      current: function () {
        return shellGen === treeGen && version === optionsVersion && treeContentGuard.isCurrent(gen);
      },
    };
    const focusInTree = !!(opts && opts.focusInTree);
    const current = currentPathFromState();
    const revealCurrent = !!(opts && opts.revealCurrent);
    const extra = opts && Array.isArray(opts.expandedPaths) ? opts.expandedPaths : [];
    // 各路径内部依次补祖先, 独立分支并发恢复; 一个慢/失败分支不阻塞其他分支.
    const targets = new Set();
    if (revealCurrent && current !== null && current !== '') targets.add(current);
    extra.concat(treeExpandedPaths).forEach(function (path) {
      if (typeof path !== 'string' || !isValidEncodedPath(path) || path === '') return;
      targets.add(path);
    });
    return Promise.all(
      Array.from(targets, function (path) {
        return walkTreePath(path, ownership, !revealCurrent || path !== current);
      })
    )
      .then(function () {
        if (!ownership.current()) return;
        clearTreeCurrent();
        const target = currentPathFromState();
        if (target !== null) {
          markTreePath(target === '' ? [] : target.split('/'));
        }
        restoreTreeSelection();
        if (focusInTree && treeRootUl && !treeRootUl.contains(document.activeElement)) {
          focusTreeRootUl();
          if (treeSelectedEl && treeRootUl) {
            treeRootUl.setAttribute('aria-activedescendant', treeSelectedEl.id);
          }
        }
      });
  }

  // 手动重新展开时恢复已记住的子树; 不开启内容导航代次, 不重选或改变打开内容.
  function restoreTreeChildExpansions(li) {
    if (!li.isConnected || li.closest('.cb-tree-node[aria-expanded="false"]')) return;
    const ul = li.querySelector(':scope > ul.cb-tree-children');
    if (!ul) return;
    $$(':scope > li', ul).forEach(function (child) {
      const path = encodedPathOf(child);
      if (!isExpandableDir(child) || path === null || !treeExpandedPaths.includes(path)) return;
      // 恢复展开不自动重试错误分支; 错误保留给用户显式激活.
      if (child.getAttribute('data-cb-branch-state') === 'error') return;
      loadTreeBranch(child, path, null).then(function (installed) {
        if (installed) restoreTreeChildExpansions(child);
      });
    });
  }

  function toggleTreeNode(li) {
    if (!isExpandableDir(li)) return;
    const path = encodedPathOf(li);
    if (path === null) return;
    const ul = li.querySelector(':scope > ul.cb-tree-children');
    const expanded = !!ul && !ul.hidden;
    if (treePending.has(path)) {
      // 在途: 按最新显式意图翻转 (第二次激活表达与上一次相反的终态); 安装方
      // 消费记录决定落地终态, 点击绝不落空也不产生第二个请求.
      const prior = treeExpandIntent.get(path);
      const next = prior === undefined ? !expanded : !prior;
      treeExpandIntent.set(path, next);
      setTreeNodeExpanded(li, next, true);
      loadTreeBranch(li, path, null).then(function (installed) {
        if (installed) restoreTreeChildExpansions(li);
      });
      return;
    }
    if (expanded) {
      treeExpandIntent.delete(path); // 无在途: 意图无消费方, 不留残留
      setTreeNodeExpanded(li, false, true);
      return;
    }
    // 展开: 失败状态允许用户手动重试 (非自动重试); 其余走当前版本缓存/在途.
    if (li.getAttribute('data-cb-branch-state') === 'error') {
      treeBranchCache.delete(path);
    }
    treeExpandIntent.set(path, true);
    setTreeNodeExpanded(li, true, true);
    loadTreeBranch(li, path, null).then(function (installed) {
      if (installed) restoreTreeChildExpansions(li);
    });
  }

  function navigateTreeNode(li) {
    const url = nodeHref(li);
    if (url === null) return;
    const path = encodedPathOf(li);
    // PDF 文件: 不 SPA fetch / 不 history / 不换 pane — 新 tab 原生查看器
    // (Chromium 内置查看器拒绝 document-sandbox 响应; 面板承载不了原生 viewer)
    if (!isDirNode(li) && isPdfFile(path, 'file')) {
      window.open(browseActionUrl(path, 'full').href, '_blank', 'noopener');
      return;
    }
    // 树内导航: SPA (fragment 注入 + pushState); 无 pane (异常) 时兜底整页导航.
    if (pane && path !== null && path !== '') {
      paneNavigate({
        kind: isDirNode(li) ? 'directory' : 'reading',
        path: path,
        history: 'push',
      });
      return;
    }
    location.href = url.href;
  }
  function treeWidthMax() {
    return Math.min(TREE_MAX_WIDTH, Math.floor(window.innerWidth * 0.5));
  }
  function clampTreePreferred(width) {
    return Math.max(TREE_MIN_WIDTH, Math.min(TREE_MAX_WIDTH, Math.round(width)));
  }
  function clampTreeWidth(width) {
    return Math.max(TREE_MIN_WIDTH, Math.min(treeWidthMax(), Math.round(width)));
  }
  function loadTreePreferredWidth() {
    let saved = 0;
    try {
      saved = Number(localStorage.getItem(TREE_WIDTH_KEY));
    } catch (err) {
      /* 持久化读取失败: 用默认值 */
    }
    return Number.isFinite(saved) && saved > 0 ? clampTreePreferred(saved) : TREE_DEFAULT_WIDTH;
  }
  function setTreeWidth(width, persist) {
    treeWidth = clampTreeWidth(width);
    document.body.style.setProperty('--cb-tree-width', treeWidth + 'px');
    if (treeResizer) {
      treeResizer.setAttribute('aria-valuemax', String(treeWidthMax()));
      treeResizer.setAttribute('aria-valuenow', String(treeWidth));
    }
    if (persist) {
      treePreferredWidth = clampTreePreferred(width);
      try {
        localStorage.setItem(TREE_WIDTH_KEY, String(treePreferredWidth));
      } catch (err) {
        /* 持久化失败忽略 */
      }
    }
  }

  /* ---------- 树壳构建 + 窄屏抽屉/折叠/宽度 ---------- */

  // 进入树壳: 清除内存 token 与全部 native 控制 (树无本机打开能力; 退出补能力
  // 只能整页重载重新取得). token 绝不进入 fragment/快照/持久存储.
  function clearNativeCapability() {
    nativeToken = null;
    const tokenMeta = document.querySelector('meta[name="cb-native-open-token"]');
    if (tokenMeta) tokenMeta.remove();
    $$('[data-cb-action="native"]').forEach(function (el) {
      el.remove();
    });
  }

  function buildTreeShell(revealCurrent) {
    if (treeShell) return;
    treeGen += 1; // 新壳: 旧壳在途响应全部失效
    clearNativeCapability();
    // 完整页 spy 观察原正文: 内容区 fragment 替换后目标脱离 DOM — 先断开.
    if (fullPageSpyDisposer) {
      fullPageSpyDisposer();
      fullPageSpyDisposer = null;
    }
    closeTreeDrawer();
    document.body.classList.add('cb-tree-mode');
    treePreferredWidth = loadTreePreferredWidth();
    if (!window.matchMedia(TREE_DRAWER_MQL).matches) {
      setTreeWidth(treePreferredWidth, false);
    }
    treeCollapsePreferred = false;
    try {
      treeCollapsePreferred = localStorage.getItem(TREE_COLLAPSED_KEY) === '1';
    } catch (err) {
      /* 持久化读取失败: 默认展开 */
    }
    installTreeCollapseMql();
    document.addEventListener('focusin', onTreeFocusIn, true);
    document.addEventListener('pointerdown', onTreePointerDown, true);
    installTreeViewportResizeHandler();

    treeShell = document.createElement('nav');
    treeShell.className = 'cb-tree';
    treeShell.id = 'cb-tree-nav'; // 稳定 id: 抽屉唤起按钮 aria-controls 锚点
    treeShell.setAttribute('aria-label', '目录树');
    // 顶栏只承载折叠控制: 搜索与显示规则控件统一在页面顶部工具区.
    const toolbar = document.createElement('div');
    toolbar.className = 'cb-tree-toolbar';
    treeCollapseBtn = document.createElement('button');
    treeCollapseBtn.type = 'button';
    treeCollapseBtn.className = 'cb-tree-collapse';
    treeCollapseBtn.setAttribute('aria-label', '收起目录树');
    treeCollapseBtn.title = '收起目录树';
    treeCollapseBtn.textContent = '‹';
    treeCollapseBtn.addEventListener('click', function (event) {
      event.stopPropagation(); // 不落入 document 全局 click
      setTreeCollapsePreference(true, true);
    });
    toolbar.append(treeCollapseBtn);
    treeShell.append(toolbar);
    treeItemSeq = 0; // 新壳: id 计数器复位 (id 仅需壳内稳定唯一)
    treeRootUl = document.createElement('ul');
    treeRootUl.className = 'cb-tree-root';
    treeRootUl.id = 'cb-tree-root'; // 稳定 id: 树根 (单 tab 停靠点) 身份锚点
    treeRootUl.setAttribute('role', 'tree');
    treeRootUl.setAttribute('aria-label', '目录树');
    treeRootUl.tabIndex = 0; // 单 tab 停靠点; 活动后代经 aria-activedescendant 公告
    treeShell.append(treeRootUl);
    document.body.prepend(treeShell);
    buildTreeResizer();
    // 窄屏抽屉配套: 遮罩 + 唤起按钮 (宽屏 display:none, 无行为).
    treeScrim = document.createElement('div');
    treeScrim.className = 'cb-tree-scrim';
    treeScrim.setAttribute('aria-hidden', 'true');
    treeScrim.addEventListener('click', closeTreeDrawer);
    document.body.append(treeScrim);
    treeOpenBtn = document.createElement('button');
    treeOpenBtn.type = 'button';
    treeOpenBtn.className = 'cb-tree-open';
    treeOpenBtn.setAttribute('aria-label', '打开目录树');
    treeOpenBtn.setAttribute('aria-controls', 'cb-tree-nav');
    treeOpenBtn.setAttribute('aria-expanded', 'false');
    treeOpenBtn.title = '打开目录树';
    treeOpenBtn.textContent = '☰ 目录';
    treeOpenBtn.addEventListener('click', openTreeDrawer);
    document.body.append(treeOpenBtn);
    // 左缘恢复按钮 (仅折叠生效时可见): 点击恢复偏好, 立即重放生效态.
    treeRestoreBtn = document.createElement('button');
    treeRestoreBtn.type = 'button';
    treeRestoreBtn.className = 'cb-tree-restore';
    treeRestoreBtn.setAttribute('aria-label', '展开目录树');
    treeRestoreBtn.title = '展开目录树';
    treeRestoreBtn.textContent = '›';
    treeRestoreBtn.hidden = true; // applyTreeCollapsed 在壳就位后同步
    treeRestoreBtn.addEventListener('click', function (event) {
      event.stopPropagation(); // 不落入 document 全局 click
      setTreeCollapsePreference(false, true);
    });
    document.body.append(treeRestoreBtn);
    installDrawerMql();
    applyTreeCollapsed(); // 壳就位: 同步生效态到新元素 (inert/hidden/类)
    // 内容区: 包装当前完整页内容进 .cb-pane, 再取当前内容区目标的 fragment
    // (目录/搜索/文档), 替换为统一 fragment 视图 (与根加载并行).
    createPane();
    clearTreeCurrent();
    refreshPaneForState({ history: 'none' });
    loadTreeRoot({ revealCurrent: revealCurrent });
  }

  // 进入/恢复 tree 视图 (唯一入口; 幂等): 新壳构建; 已有壳只重同步 - 内容区按
  // 快照目标重取 (已成功安装同一目标则不重取), 树标记 current 并恢复选中 identity.
  function ensureTreeShell(revealCurrent) {
    installBrowseTopbarReplacement(); // 工具区交接先于控件同步 (树壳无阅读导航)
    syncControlState();
    if (!treeShell) {
      buildTreeShell(revealCurrent);
      return;
    }
    clearTreeCurrent();
    syncTreeToState(true);
    restoreTreeSelection();
    refreshPaneForState({ history: 'none' });
  }

  function buildTreeResizer() {
    treeResizer = document.createElement('div');
    treeResizer.className = 'cb-tree-resizer';
    treeResizer.tabIndex = 0;
    treeResizer.setAttribute('role', 'separator');
    treeResizer.setAttribute('aria-orientation', 'vertical');
    treeResizer.setAttribute('aria-label', '调整目录树宽度');
    treeResizer.setAttribute('aria-valuemin', String(TREE_MIN_WIDTH));
    treeResizer.setAttribute('aria-valuemax', String(treeWidthMax()));
    treeResizer.setAttribute('aria-valuenow', String(treeWidth));
    treeResizer.addEventListener('pointerdown', function (event) {
      if (event.target.closest('.cb-tree-collapse')) return;
      if (!canStartTreeResize(event, treeResizePointerId)) return;
      try {
        treeResizer.setPointerCapture(event.pointerId);
      } catch (err) {
        return; // 捕获失败 (异常/禁用) 干净返回, 不留 class/监听/持久化
      }
      const pointerId = event.pointerId;
      const startX = event.clientX;
      const startWidth = treeWidth;
      let dragWidth = startWidth; // 未截断的目标宽: 结束时按硬边界持久化
      treeResizePointerId = pointerId;
      document.body.classList.add('cb-tree-resizing');
      if (event.cancelable) event.preventDefault();
      const onMove = function (moveEvent) {
        if (!ownsTreeResize(moveEvent, treeResizePointerId)) return;
        dragWidth = startWidth + moveEvent.clientX - startX;
        setTreeWidth(dragWidth, false);
        if (moveEvent.cancelable) moveEvent.preventDefault();
      };
      // 幂等收尾: 任意结束信号 (up/cancel/lostpointercapture) 只清理一次.
      const onEnd = function (endEvent) {
        if (!ownsTreeResize(endEvent, treeResizePointerId)) return;
        treeResizePointerId = null;
        treeResizer.removeEventListener('pointermove', onMove);
        treeResizer.removeEventListener('pointerup', onEnd);
        treeResizer.removeEventListener('pointercancel', onEnd);
        treeResizer.removeEventListener('lostpointercapture', onEnd);
        document.body.classList.remove('cb-tree-resizing');
        try {
          treeResizer.releasePointerCapture(pointerId);
        } catch (err) {
          /* 指针捕获已隐式释放: 忽略 */
        }
        setTreeWidth(dragWidth, true); // 持久化按硬边界截断, 不写 50vw 截断值
      };
      treeResizer.addEventListener('pointermove', onMove);
      treeResizer.addEventListener('pointerup', onEnd);
      treeResizer.addEventListener('pointercancel', onEnd);
      treeResizer.addEventListener('lostpointercapture', onEnd);
    });
    treeResizer.addEventListener('keydown', function (event) {
      if (event.target.closest('.cb-tree-collapse')) return;
      let next = null;
      switch (event.key) {
        case 'ArrowLeft':
          next = treeWidth - 16;
          break;
        case 'ArrowRight':
          next = treeWidth + 16;
          break;
        case 'Home':
          next = TREE_MIN_WIDTH;
          break;
        case 'End':
          next = treeWidthMax();
          break;
        default:
          return;
      }
      event.preventDefault();
      event.stopPropagation(); // 阻断冒泡: 分隔器按键不得落入全局树键
      setTreeWidth(next, true);
    });
    document.body.append(treeResizer);
  }

  /* ---------- 窄屏抽屉 (<64rem): off-canvas transform + 遮罩; 宽屏恒 no-op ---------- */

  function openTreeDrawer() {
    document.body.classList.add('cb-drawer-open');
    if (treeOpenBtn) {
      treeOpenBtn.setAttribute('aria-expanded', 'true');
    }
    const focusTarget = treeRootUl || treeShell;
    if (focusTarget) {
      try {
        focusTarget.focus({ preventScroll: true });
      } catch (err) {
        /* 老环境不支持 focus options: 忽略 */
      }
    }
  }

  function closeTreeDrawer() {
    const wasOpen = document.body.classList.contains('cb-drawer-open');
    document.body.classList.remove('cb-drawer-open');
    if (treeOpenBtn) {
      treeOpenBtn.setAttribute('aria-expanded', 'false');
    }
    if (wasOpen && treeShell && treeShell.contains(document.activeElement)) {
      if (treeOpenBtn) {
        treeOpenBtn.focus({ preventScroll: true });
      } else {
        document.activeElement.blur();
      }
    }
  }

  function installDrawerMql() {
    drawerMql = window.matchMedia(TREE_DRAWER_MQL);
    drawerMqlHandler = function (e) {
      if (!e.matches) {
        closeTreeDrawer();
        setTreeWidth(treePreferredWidth, false);
      }
    };
    if (drawerMql.addEventListener) {
      drawerMql.addEventListener('change', drawerMqlHandler);
    } else if (drawerMql.addListener) {
      drawerMql.addListener(drawerMqlHandler);
    }
  }

  function installTreeViewportResizeHandler() {
    treeViewportResizeHandler = function () {
      if (window.matchMedia(TREE_DRAWER_MQL).matches) return;
      setTreeWidth(treePreferredWidth, false);
    };
    window.addEventListener('resize', treeViewportResizeHandler);
  }

  // 焦点落点纯函数: 返回 'restore' | 'collapse' | 'drawer' | null.
  function collapseFocusTarget(nextCollapsed, narrow, activeKind) {
    if (
      nextCollapsed &&
      (activeKind === 'tree' || activeKind === 'resizer' || activeKind === 'drawer')
    ) {
      return 'restore';
    }
    if (!nextCollapsed && activeKind === 'restore') {
      return narrow ? 'drawer' : 'collapse';
    }
    if (!nextCollapsed && !narrow && activeKind === 'drawer') {
      return 'collapse';
    }
    return null;
  }

  // 树壳焦点分类: 树内 → 'tree', 调整器内 → 'resizer', 恢复按钮自身 → 'restore',
  // 抽屉唤起按钮自身 → 'drawer', 其余 → null.
  function classifyTreeFocusTarget(target) {
    if (treeShell && treeShell.contains(target)) return 'tree';
    if (treeResizer && treeResizer.contains(target)) return 'resizer';
    if (treeRestoreBtn && target === treeRestoreBtn) return 'restore';
    if (treeOpenBtn && target === treeOpenBtn) return 'drawer';
    return null;
  }

  function onTreeFocusIn(event) {
    treeFocusKind = classifyTreeFocusTarget(event.target);
  }

  function onTreePointerDown(event) {
    if (!classifyTreeFocusTarget(event.target)) {
      treeFocusKind = null;
    }
  }

  function applyTreeCollapsed() {
    if (!treeShell) return;
    const narrow = treeCollapseMql
      ? treeCollapseMql.matches
      : window.matchMedia(TREE_DRAWER_MQL).matches;
    const nextCollapsed = treeCollapsePreferred && !narrow;
    const active = document.activeElement;
    let activeKind = active ? classifyTreeFocusTarget(active) : null;
    if (!activeKind && treeFocusKind) {
      activeKind = treeFocusKind;
    }
    const focusTarget = collapseFocusTarget(nextCollapsed, narrow, activeKind);
    treeCollapsed = nextCollapsed;
    if (treeCollapsed) {
      if (treeRestoreBtn) treeRestoreBtn.hidden = false;
      document.body.classList.add('cb-tree-collapsed');
      if (focusTarget === 'restore' && treeRestoreBtn) {
        treeRestoreBtn.focus({ preventScroll: true });
        treeFocusKind = 'restore';
      }
      if (treeShell) {
        treeShell.inert = true;
        treeShell.setAttribute('aria-hidden', 'true');
      }
      if (treeResizer) treeResizer.hidden = true;
      return;
    }
    document.body.classList.remove('cb-tree-collapsed');
    if (treeShell) {
      treeShell.inert = false;
      treeShell.setAttribute('aria-hidden', 'false');
    }
    if (treeResizer) treeResizer.hidden = false;
    if (focusTarget === 'drawer') {
      if (treeOpenBtn) {
        treeOpenBtn.focus({ preventScroll: true });
        treeFocusKind = 'drawer';
      }
    } else if (focusTarget === 'restore' || focusTarget === 'collapse') {
      if (treeCollapseBtn) {
        treeCollapseBtn.focus({ preventScroll: true });
        treeFocusKind = 'tree';
      } else if (treeResizer) {
        treeResizer.focus({ preventScroll: true });
        treeFocusKind = 'resizer';
      } else if (treeRootUl) {
        treeRootUl.focus({ preventScroll: true });
        treeFocusKind = 'tree';
      } else if (treeShell) {
        treeShell.focus({ preventScroll: true });
        treeFocusKind = 'tree';
      }
    }
    if (treeRestoreBtn) treeRestoreBtn.hidden = true;
  }

  // 折叠偏好: persist 只写两态 '1'/'0' (仅按钮交互写入).
  function setTreeCollapsePreference(preferred, persist) {
    treeCollapsePreferred = preferred;
    if (persist) {
      try {
        localStorage.setItem(TREE_COLLAPSED_KEY, preferred ? '1' : '0');
      } catch (err) {
        /* 持久化失败忽略 */
      }
    }
    applyTreeCollapsed();
  }

  function installTreeCollapseMql() {
    treeCollapseMql = window.matchMedia(TREE_DRAWER_MQL);
    treeCollapseMqlHandler = applyTreeCollapsed;
    if (treeCollapseMql.addEventListener) {
      treeCollapseMql.addEventListener('change', treeCollapseMqlHandler);
    } else if (treeCollapseMql.addListener) {
      treeCollapseMql.addListener(treeCollapseMqlHandler);
    }
  }

  /* ---------- 内容区 (tree 视图的 SPA 内容面板) ---------- */

  // 内容区: tree 视图右侧滚动容器. 进入树壳时把当前完整页内容 (目录页
  // main.cb-main / 文档页 .cb-doc) 包装进 .cb-pane, 再 fetch 当前内容区目标的
  // fragment 替换为统一 fragment 视图 (目录/搜索/文档). 树内导航全部 SPA:
  // fragment 注入 + history 快照; URL 恒为含有效规则的浏览上下文 URL.
  let pane = null;
  let paneAbort = null; // 在途 fragment fetch 的 AbortController
  let paneCommittedKey = null; // 最近一致且成功安装的内容键 (占位/失败不记)
  let paneCommittedTarget = null; // 最近提交的目标 {kind,path,query}
  const paneNavGuard = createGenerationGuard();
  // pane 生命周期 registry: 内容区 fragment 替换时统一拆除.
  const paneDisposables = [];

  function registerPaneDisposable(fn) {
    if (typeof fn === 'function') paneDisposables.push(fn);
  }

  // 卸载 pane 内媒体: 音视频暂停并移除 (停止播放与资源加载).
  function unloadPaneMedia() {
    if (!pane) return;
    pane.querySelectorAll('audio, video').forEach(function (el) {
      try {
        el.pause();
      } catch (err) {
        /* 已分离等场景忽略 */
      }
      el.remove();
    });
  }

  function teardownPane() {
    paneDisposables.splice(0).forEach(function (fn) {
      try {
        fn();
      } catch (err) {
        /* 单个 teardown 失败不阻断其余 */
      }
    });
    unloadPaneMedia();
  }

  // 被包装的完整页内容宿主: 目录页 main.cb-main, 文档页 .cb-doc.
  function paneHostForPage() {
    return document.querySelector('main.cb-main') || document.querySelector('.cb-doc');
  }

  function createPane() {
    if (pane) return pane;
    const host = paneHostForPage();
    if (!host) return null;
    const wrap = document.createElement('div');
    wrap.className = 'cb-pane';
    wrap.setAttribute('role', 'region');
    wrap.setAttribute('aria-label', '内容区');
    wrap.setAttribute('data-cb-pane-state', 'loading');
    host.before(wrap);
    wrap.append(host);
    pane = wrap;
    return wrap;
  }

  function paneSetState(status) {
    if (!pane) return;
    pane.setAttribute('data-cb-pane-state', status);
  }

  function paneClearError() {
    if (!pane) return;
    const errEl = pane.querySelector(':scope > .cb-pane-error');
    if (errEl) errEl.remove();
  }

  // 错误只经 textContent (绝不 innerHTML): 保留先前打开内容, URL 不变.
  function paneShowError(message) {
    if (!pane) return;
    let errEl = pane.querySelector(':scope > .cb-pane-error');
    if (!errEl) {
      errEl = document.createElement('div');
      errEl.className = 'cb-pane-error';
      errEl.setAttribute('role', 'alert');
      pane.prepend(errEl);
    }
    errEl.textContent = message;
  }

  // 当前版本的目标键 (目录/搜索含显示规则; 文档正文不含).
  function paneTargetKey(target) {
    return contentKey(target.kind, target.kind === 'search' ? '' : target.path, target.query);
  }

  function paneFragmentUrl(kind, target) {
    if (kind === 'search') {
      const qParams = new URLSearchParams();
      qParams.set('q', target.query || '');
      return new URL(
        '/__/search?' +
          qParams.toString() +
          '&fragment=1&sort=' +
          state.rules.sort +
          '&hidden=' +
          (state.rules.hidden ? '1' : '0'),
        location.origin
      );
    }
    if (kind === 'directory') {
      const base = target.path === '' ? '/' : '/' + target.path;
      return new URL(
        base +
          '?fragment=1&sort=' +
          state.rules.sort +
          '&hidden=' +
          (state.rules.hidden ? '1' : '0'),
        location.origin
      );
    }
    return browseActionUrl(target.path, 'fragment');
  }

  function paneTargetPathname() {
    return state.kind === 'search' ? '/__/search' : '/' + state.path;
  }

  // 真实导航: 新增一个携带当前快照的目标项 (hash 只在同文档延续).
  function pushPaneUrl() {
    const samePath = locationPathname() === paneTargetPathname();
    const url = contextUrl(state.kind, state.path, state.q, samePath ? location.hash : '');
    if (location.href !== url.href) pushEntry(url);
  }

  function paneFail(message) {
    paneCommittedKey = null; // 失败使成功标记失效
    paneCommittedTarget = null;
    paneSetState('error');
    paneShowError(message);
  }

  // 内容区导航: fetch fragment -> 校验 (ok + text/html + wrapper 语义/identity)
  // -> 提交 (teardown 旧 pane 状态, 安装新内容, 按需写历史/快照, 树状态同步).
  // 失败 fail-closed: 保留 pane 内容, 不提交目标历史/上下文, 文本错误.
  function paneNavigate(target) {
    if (!pane || !target) return null;
    const kind =
      target.kind === 'search' ? 'search' : target.kind === 'reading' ? 'reading' : 'directory';
    // 目录/搜索: 合法 encoded path ('' = 服务根, 是合法浏览目标); 文档: 必须非空.
    if (!isValidEncodedPath(target.path)) return null;
    if (kind === 'reading' && target.path === '') return null;
    const key = paneTargetKey({ kind: kind, path: target.path, query: target.query });
    const gen = paneNavGuard.begin(); // 新导航作废所有在途导航
    if (paneAbort) paneAbort.abort();
    const ac = new AbortController();
    paneAbort = ac;
    const sameCommitted =
      paneCommittedKey === key &&
      paneCommittedTarget !== null &&
      paneCommittedTarget.kind === kind &&
      paneCommittedTarget.path === target.path;
    if (sameCommitted) {
      // 一致且成功安装: 不重取; 真实导航只补写 URL/快照 (hash 导航等).
      paneClearError();
      paneSetState('ready');
      if (target.history === 'push') pushPaneUrl();
      return gen;
    }
    paneClearError();
    paneSetState('loading');
    fetch(paneFragmentUrl(kind, target), {
      headers: { Accept: 'text/html' },
      signal: ac.signal,
    })
      .then(function (res) {
        if (!paneNavGuard.isCurrent(gen)) return null; // 过期: 丢弃
        return readFragmentResponse(res);
      })
      .then(function (html) {
        if (html === null || !paneNavGuard.isCurrent(gen)) return;
        commitPane(html, target, kind, gen);
      })
      .catch(function (err) {
        if (!paneNavGuard.isCurrent(gen)) return; // 过期错误: 丢弃
        if (err && err.name === 'AbortError') return; // 被新导航中止: 不报错
        paneFail(
          '无法加载：' + (err && err.message ? err.message : String(err))
        );
      });
    return gen;
  }

  // 提交守卫: generation 最新 + wrapper 语义/identity 与请求目标一致 (fail-closed)
  // -> 才动 DOM/状态/历史. wrapper 回显的有效规则也必须与请求规则一致.
  function commitPane(html, target, kind, gen) {
    if (!paneNavGuard.isCurrent(gen) || !pane) return;
    const host = document.createElement('div');
    host.innerHTML = html; // 服务端 fragment (与预览面板同一信任契约)
    const wrapper = findFragmentWrapper(host);
    if (!wrapper) {
      paneFail('无法加载：响应内容为空');
      return;
    }
    const surface = wrapperSurface(wrapper);
    const wrapperIdentity =
      typeof wrapper.dataset.nativePathEncoded === 'string'
        ? wrapper.dataset.nativePathEncoded
        : null;
    let consistent = false;
    if (kind === 'search') {
      consistent =
        surface === 'search' && wrapperIdentity === '' &&
        wrapper.dataset.cbQuery === (target.query || '');
    } else {
      const requestedKey = canonicalTransportKey(target.path);
      const wrapperKey =
        wrapperIdentity === null ? null : canonicalTransportKey(wrapperIdentity);
      consistent =
        surface === kind &&
        requestedKey !== null &&
        wrapperKey !== null &&
        requestedKey === wrapperKey &&
        isValidEncodedPath(wrapperIdentity);
    }
    if (consistent && kind !== 'reading') {
      // 旧集合不得冒充新规则结果: wrapper 回显规则不一致视为内容不匹配.
      const echoSort = wrapper.dataset.cbSort;
      const echoHidden = wrapper.dataset.cbHidden;
      if (echoSort !== state.rules.sort || echoHidden !== (state.rules.hidden ? '1' : '0')) {
        consistent = false;
      }
    }
    if (!consistent) {
      paneFail('无法加载：响应内容与目标不符');
      return;
    }
    teardownPane(); // 旧 observer 拆除 + 媒体卸载
    paneClearError();
    pane.replaceChildren(wrapper); // 移动解析出的 wrapper (替换全部旧内容)
    const titleEl = wrapper.querySelector('.cb-frag-title');
    if (titleEl && titleEl.textContent) {
      document.title = titleEl.textContent.trim();
    }
    pane.scrollTop = 0;
    mirrorContentState(pane, wrapper);
    // 文档布局: wrapper 即 .cb-doc (文档片段) 时直接装; 否则查内层 .cb-doc.
    const docEl = wrapper.matches('.cb-doc') ? wrapper : wrapper.querySelector('.cb-doc');
    installDocLayout(docEl);
    if (docEl) installPaneTocSpy(docEl);
    renderMermaidIn(wrapper); // 内容区 fragment 内的 mermaid 容器
    // 请求目标与成功安装内容分开记录: 占位/忙内容不记为可跳过重取的成功.
    const placeholder = wrapperIsPlaceholder(wrapper);
    paneCommittedTarget = { kind: kind, path: target.path, query: target.query };
    paneCommittedKey = placeholder ? null : paneTargetKey(paneCommittedTarget);
    paneSetState('ready');
    state.kind = kind;
    state.path = kind === 'search' ? '' : target.path;
    state.q = kind === 'search' ? target.query || '' : null;
    // 同上下文刷新由树根恢复展开/选中; 不另开树代次抢走在途分支提交权.
    if (target.history === 'push') {
      pushPaneUrl();
      syncTreeToState(true); // 只有实际导航让选中跟随新的内容路径
    }
  }

  // 按当前状态同步内容区 (进入树壳/恢复/规则变化后的重取入口).
  function refreshPaneForState(opts) {
    if (!pane) return;
    const history = opts && opts.history ? opts.history : 'none';
    if (state.kind === 'search') {
      paneNavigate({ kind: 'search', path: '', query: state.q, history: history });
      return;
    }
    paneNavigate({ kind: state.kind, path: state.path, history: history });
  }

  // 显示规则变化: 目录/搜索内容依赖规则 → 重取; 文档正文绝不重转换.
  function refreshPaneForRules() {
    if (!pane) return;
    if (state.kind === 'reading') return;
    refreshPaneForState({ history: 'none' });
  }

  // 同步树状态到当前内容区目标: 展开路径链, 标记 current, 用户导航时选中跟随.
  function syncTreeToState(select) {
    if (!treeRootUl) return Promise.resolve();
    const shellGen = treeGen;
    const gen = treeContentGuard.begin();
    const ownership = {
      required: true,
      current: function () {
        return shellGen === treeGen && treeContentGuard.isCurrent(gen);
      },
    };
    const target = currentPathFromState();
    const chain = target === null ? Promise.resolve(null) : walkTreePath(target, ownership);
    return chain.then(function () {
      if (!ownership.current()) return;
      clearTreeCurrent();
      if (target !== null) {
        markTreePath(target === '' ? [] : target.split('/'));
      }
      if (select) {
        if (target === null) return; // 搜索上下文: 保留当前选中
        const node = findVisibleTreeNode(state.path);
        if (node) {
          selectTreeNode(node, { silent: true });
        } else {
          clearTreeSelection({ silent: true });
        }
      }
    });
  }

  /* ---------- TOC scroll-spy ---------- */

  // TOC 容器: markdown/org 自渲染为 #TOC; #table-of-contents 为 pandoc 兼容形态.
  function tocNavIn(root) {
    return root.querySelector('#TOC, #table-of-contents');
  }

  // 文档布局: TOC 与正文是 .cb-doc 下的真实兄弟列. 幂等 — 已装过直接返回现有
  // layout. 原 #TOC / #table-of-contents 节点被移动 (绝不克隆), 锚点 id / href /
  // slug 契约不变. 无 TOC 文档返回 null, 不建 wrapper.
  function installDocLayout(docEl) {
    if (!docEl) return null;
    const existing = docEl.querySelector(':scope > .cb-doc-layout');
    if (existing) return existing;
    const tocNav = tocNavIn(docEl);
    if (!tocNav) return null;
    const originalNodes = Array.from(docEl.childNodes);
    const layout = document.createElement('div');
    layout.className = 'cb-doc-layout';
    const tocHost = document.createElement('aside');
    tocHost.className = 'cb-doc-toc';
    tocHost.setAttribute('aria-label', '文档目录');
    const content = document.createElement('div');
    content.className = 'cb-doc-content';
    tocHost.append(tocNav); // 原节点移动: slug/href 配对与标题锚点不变
    originalNodes.forEach(function (node) {
      if (node !== tocNav) content.append(node);
    });
    layout.append(tocHost, content); // TOC-first DOM (a11y / 窄屏阅读序)
    docEl.replaceChildren(layout);
    return layout;
  }

  // 通用 scroll-spy: 观察被 TOC 收录的标题锚点 (hN > a[id] 且 id 出现在 TOC
  // href 中); 当前标题 = 阅读线 (root 顶部 20%) 之上且 top 最大 (文档序最靠后)
  // 的已收录标题. IntersectionObserver 只在任一标题越过阅读线时唤醒重算, 重算
  // 为廉价的 rect 全量扫描. rootEl 传 null 以视口为根 (完整页), 传元素以该
  // 滚动容器为根 (pane). 返回 disposer (observer disconnect + scroll 监听移除),
  // 调用方决定注册进哪个生命周期; 窄屏 (无 rail) / 无收录标题返回 null.
  function installTocSpy(docEl, tocNav, rootEl) {
    if (!window.matchMedia('(min-width: 75rem)').matches) return null;
    const links = Object.create(null);
    $$('a[href^="#"]', tocNav).forEach(function (a) {
      const slug = a.getAttribute('href').slice(1);
      if (slug !== '') links[slug] = a;
    });
    const targets = [];
    $$('h1, h2, h3, h4, h5, h6', docEl).forEach(function (h) {
      const anchor = h.querySelector(':scope > a[id]');
      if (anchor && links[anchor.id]) targets.push(anchor);
    });
    if (targets.length === 0) return null; // 无收录标题: 不启用 spy
    let currentLink = null;
    function setCurrent(link) {
      if (link === currentLink) return;
      if (currentLink) currentLink.classList.remove('cb-toc-current');
      currentLink = link;
      if (link) link.classList.add('cb-toc-current');
    }
    function rootAtMaxScroll() {
      if (!rootEl) {
        return (
          window.scrollY + window.innerHeight >=
          document.documentElement.scrollHeight - 1
        );
      }
      return rootEl.scrollTop + rootEl.clientHeight >= rootEl.scrollHeight - 1;
    }
    function rootScrollable() {
      if (!rootEl) {
        return document.documentElement.scrollHeight > window.innerHeight + 1;
      }
      return rootEl.scrollHeight > rootEl.clientHeight + 1;
    }
    function compute() {
      const rootTop = rootEl ? rootEl.getBoundingClientRect().top : 0;
      const rootHeight = rootEl
        ? rootEl.getBoundingClientRect().height
        : window.innerHeight;
      // 阅读线为 root 相对坐标 (与标题 top 同一坐标系, 不混视口绝对坐标)
      const line = rootHeight * 0.2;
      const last = targets[targets.length - 1];
      if (!rootScrollable()) {
        setCurrent(links[targets[0].id] || null); // 短文档一屏放得下: 高亮首个标题
        return;
      }
      if (rootAtMaxScroll()) {
        setCurrent(links[last.id] || null); // 滚动到底: 末节即当前节
        return;
      }
      let best = null; // { anchor, top }
      targets.forEach(function (a) {
        const top = a.getBoundingClientRect().top - rootTop;
        if (top <= line && (best === null || top > best.top)) {
          best = { anchor: a, top: top };
        }
      });
      if (
        best === null &&
        last.getBoundingClientRect().bottom - rootTop <= rootHeight
      ) {
        best = { anchor: targets[0], top: -1 };
      }
      setCurrent(best === null ? null : links[best.anchor.id] || null);
    }
    const observer = new IntersectionObserver(compute, {
      root: rootEl || null,
      rootMargin: '0px 0px -80% 0px', // 观察带 = root 顶部 20%
      threshold: 0,
    });
    targets.forEach(function (a) {
      observer.observe(a);
    });
    const scrollRoot = rootEl || window;
    scrollRoot.addEventListener('scroll', compute, { passive: true });
    compute(); // 初始状态 (深链已滚动/短文档直接出结果)
    return function () {
      // 断开前清除当前高亮: 跨断点重装时旧实例的 .cb-toc-current 不得残留
      setCurrent(null);
      observer.disconnect();
      scrollRoot.removeEventListener('scroll', compute);
    };
  }

  let paneSpyDisposer = null;
  // 断点感知 spy 控制器: 跨 75rem 时按当前模式重装正确根的 spy; 窄屏恒无 spy.
  function reinstallCurrentSpy() {
    if (state.view === 'tree' && pane) {
      if (paneSpyDisposer) {
        paneSpyDisposer();
        paneSpyDisposer = null;
      }
      const docEl = pane.querySelector('.cb-doc');
      if (docEl) installPaneTocSpy(docEl);
    } else if (document.body.classList.contains('cb-doc-page')) {
      if (fullPageSpyDisposer) {
        fullPageSpyDisposer();
        fullPageSpyDisposer = null;
      }
      initFullPageTocSpy();
    }
  }

  function installTocSpyMql() {
    const mql = window.matchMedia('(min-width: 75rem)');
    const handler = function () {
      reinstallCurrentSpy();
    };
    if (mql.addEventListener) {
      mql.addEventListener('change', handler);
    } else if (mql.addListener) {
      mql.addListener(handler);
    }
  }

  // 完整页 spy (目录模式文档页): root = 视口, 页面生命周期.
  let fullPageSpyDisposer = null;
  function initFullPageTocSpy() {
    if (!document.body.classList.contains('cb-doc-page')) return;
    const docEl = document.querySelector('.cb-doc');
    installDocLayout(docEl); // 先装布局 (TOC 移入 toc 列), 再按新 DOM 装 spy
    const tocNav = docEl ? tocNavIn(docEl) : null;
    if (!tocNav) return; // 无 TOC 页面 (代码/无标题文档): 不启用 spy
    fullPageSpyDisposer = installTocSpy(docEl, tocNav, null);
  }

  function installPaneTocSpy(docEl) {
    if (!pane || !docEl) return;
    const tocNav = tocNavIn(docEl);
    if (!tocNav) return; // 无 TOC fragment (代码/目录/媒体/占位): 不启用 spy
    const disposer = installTocSpy(docEl, tocNav, pane);
    if (disposer) {
      paneSpyDisposer = disposer;
      const registeredDisposer = disposer;
      registerPaneDisposable(function () {
        if (paneSpyDisposer === registeredDisposer) {
          paneSpyDisposer = null;
        }
        registeredDisposer();
      });
    }
  }

  /* ---------- Mermaid 图表渲染 (懒加载; 事务式 render; 失败保留源码) ---------- */

  // 服务端把 md fence ```mermaid 与 org #+begin_src mermaid 渲染为
  // <pre class="mermaid">转义源码</pre> (textContent 即原始图源码)。
  // 这里发现容器才懒加载 mermaid.min.js (3.5MB, 全站只请求一次), 渲染为 SVG。
  // 降级约定 (与 math.rs 一致): 库加载失败/图语法错误 → 保留原始源码显示。
  const mermaidSources = new WeakMap(); // 元素 → 原始图源码
  const mermaidTickets = new WeakMap(); // 元素 → 渲染票号 (单调递增)
  let mermaidLoading = null; // 加载 Promise 缓存; 失败复位以允许下次重试
  let mermaidSeq = 0; // mermaid.render 要求的唯一 id 序列

  function mermaidConfig(el) {
    const style = window.getComputedStyle(document.documentElement);
    const dark =
      window.matchMedia &&
      window.matchMedia('(prefers-color-scheme: dark)').matches;
    function cbVar(name) {
      const v = style.getPropertyValue(name).trim();
      return v || undefined;
    }
    const host =
      (el && el.closest && el.closest('.cb-doc')) ||
      document.querySelector('.cb-doc') ||
      document.body;
    const fontFamily = window.getComputedStyle(host).fontFamily;
    const themeVariables = {
      // darkMode 必须在 themeVariables 内: theme-base 读 overrides.darkMode
      darkMode: !!dark,
      background: cbVar('--cb-bg'),
      primaryColor: cbVar('--cb-soft-bg'),
      primaryTextColor: cbVar('--cb-text'),
      primaryBorderColor: cbVar('--cb-border'),
      lineColor: cbVar('--cb-muted'),
      textColor: cbVar('--cb-text'),
      fontFamily: fontFamily || undefined,
    };
    return {
      startOnLoad: false, // 手动驱动; 库是懒加载的, load 事件早已过去
      securityLevel: 'strict', // 文件可来自局域网分享: 禁 click/HTML label
      suppressErrorRendering: true, // 错误图由 catch 分支的源码回退取代
      theme: 'base', // 官方自定义打底预设; 站点色板全靠 themeVariables (源自 CSS)
      themeVariables: themeVariables,
    };
  }

  function loadMermaid() {
    if (mermaidLoading !== null) return mermaidLoading;
    mermaidLoading = new Promise(function (resolve, reject) {
      const script = document.createElement('script');
      script.src = '/__/static/js/mermaid.min.js';
      script.onload = resolve;
      script.onerror = function () {
        mermaidLoading = null;
        reject(new Error('mermaid.min.js 加载失败'));
      };
      document.head.appendChild(script);
    });
    return mermaidLoading;
  }

  // mermaid.render 的工作节点 (id 为 'd'+id) 可能残留在 body 下, 清理.
  function removeMermaidStray(id) {
    const stray = document.getElementById('d' + id);
    if (stray && stray.parentNode) stray.parentNode.removeChild(stray);
  }

  // 事务式渲染: 不把容器元素交给 mermaid (v11 会先清空传入元素, 失败即丢源码),
  // 先用保存的源码 render 出 SVG, 成功、票号最新且节点仍在文档中才提交;
  // 任何失败都恢复原始源码 (suppressErrorRendering 已禁掉 mermaid 的错误图).
  function renderOneMermaid(el) {
    const source = mermaidSources.get(el);
    if (typeof source !== 'string') return;
    const ticket = (mermaidTickets.get(el) || 0) + 1;
    mermaidTickets.set(el, ticket);
    const id = 'cb-mermaid-' + mermaidSeq++;
    window.mermaid.render(id, source).then(
      function (out) {
        removeMermaidStray(id);
        if (mermaidTickets.get(el) !== ticket) return; // 已被更新的渲染取代
        if (!el.isConnected) return; // fragment 已被替换: 丢弃结果
        el.innerHTML = out.svg;
        el.classList.add('cb-mermaid-rendered');
        if (typeof out.bindFunctions === 'function') out.bindFunctions(el);
      },
      function () {
        removeMermaidStray(id);
        if (mermaidTickets.get(el) !== ticket) return;
        if (!el.isConnected) return;
        el.textContent = source; // 回退源码显示
        el.classList.remove('cb-mermaid-rendered');
      }
    );
  }

  // 扫描 root 内未渲染的 mermaid 容器并渲染; root 为 document 时覆盖全页.
  function renderMermaidIn(root) {
    if (!root || typeof root.querySelectorAll !== 'function') return;
    const els = Array.from(
      root.querySelectorAll('pre.mermaid:not(.cb-mermaid-rendered)')
    );
    if (els.length === 0) return;
    els.forEach(function (el) {
      if (!mermaidSources.has(el)) {
        mermaidSources.set(el, el.textContent || '');
      }
    });
    loadMermaid().then(
      function () {
        if (!window.mermaid) return;
        window.mermaid.initialize(mermaidConfig(els[0]));
        els.forEach(renderOneMermaid);
      },
      function () {
        /* 库加载失败: 源码保持显示; mermaidLoading 已复位, 下次扫描重试 */
      }
    );
  }

  // 系统主题切换: mermaid 主题色在渲染时烘焙进 SVG (无 CSS 变量可循),
  // 用保存的源码按新主题重渲全部仍在文档中的图.
  function rethemeMermaids() {
    if (!window.mermaid) return; // 从未加载: 下次扫描自会按新主题初始化
    window.mermaid.initialize(mermaidConfig());
    Array.from(document.querySelectorAll('pre.mermaid')).forEach(function (el) {
      el.classList.remove('cb-mermaid-rendered');
      renderOneMermaid(el);
    });
  }

  /* ---------- 键盘 ---------- */

  function isEditableTarget(el) {
    if (!el) return false;
    const tag = el.tagName;
    return (
      tag === 'INPUT' ||
      tag === 'TEXTAREA' ||
      tag === 'SELECT' ||
      el.isContentEditable
    );
  }

  function topbarSearchInput() {
    return $('#cb-search-q');
  }

  // 树键范围纯函数 (DOM 无关 seam): 返回 'escape-search' | 'focus-search' |
  // 'tree' | null.
  function treeKeyScope(key, editable, rootFocused, searchFocused) {
    if (key === 'Escape') {
      if (searchFocused) return 'escape-search';
      if (rootFocused) return 'tree';
      return null;
    }
    if (editable) return null;
    if (key === '/') return 'focus-search';
    if (
      key === 'ArrowUp' ||
      key === 'ArrowDown' ||
      key === 'ArrowLeft' ||
      key === 'ArrowRight' ||
      key === 'Enter' ||
      key === 'Backspace' ||
      key === 'PageUp' ||
      key === 'PageDown'
    ) {
      return rootFocused ? 'tree' : null;
    }
    return null;
  }

  function onTreeKeydown(event) {
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    // 分隔器按键由 resizer 自行处理并阻断冒泡; 分隔器子控件的按键同样不得落入
    // 全局树键 (defaultPrevented 兜底).
    if (event.defaultPrevented || event.target.closest('.cb-tree-resizer')) return;
    if (event.target.closest('.cb-tree-restore')) return;
    const searchInput = topbarSearchInput();
    const scope = treeKeyScope(
      event.key,
      isEditableTarget(event.target),
      !!treeRootUl && document.activeElement === treeRootUl,
      !!searchInput && document.activeElement === searchInput
    );
    if (scope === null) return; // 非树范围按键: 不取消默认, 放行原生行为
    if (scope === 'escape-search') {
      if (document.body.classList.contains('cb-drawer-open')) {
        closeTreeDrawer();
      }
      if (searchInput && document.activeElement === searchInput) {
        searchInput.blur();
      }
      event.preventDefault();
      return;
    }
    if (scope === 'focus-search') {
      if (searchInput) searchInput.focus();
      event.preventDefault();
      return;
    }
    if (event.key === 'Escape') {
      clearTreeSelection();
      closeTreeDrawer();
      event.preventDefault();
      return;
    }
    switch (event.key) {
      case 'PageUp':
        if (
          treeRootUl &&
          (event.target === treeRootUl || document.activeElement === treeRootUl)
        ) {
          if (treeShell) {
            treeShell.scrollBy({ top: -Math.round(treeShell.clientHeight * 0.8) });
          }
          event.preventDefault();
        }
        break;
      case 'PageDown':
        if (
          treeRootUl &&
          (event.target === treeRootUl || document.activeElement === treeRootUl)
        ) {
          if (treeShell) {
            treeShell.scrollBy({ top: Math.round(treeShell.clientHeight * 0.8) });
          }
          event.preventDefault();
        }
        break;
      case 'ArrowDown':
        moveTreeSelection(1);
        event.preventDefault();
        break;
      case 'ArrowUp':
        moveTreeSelection(-1);
        event.preventDefault();
        break;
      case 'ArrowRight':
        // → 只展开 (契约: → 展开 / ← 收起); 已展开则 no-op.
        if (treeSelectedEl && isExpandableDir(treeSelectedEl)) {
          const ul = treeSelectedEl.querySelector(':scope > ul.cb-tree-children');
          if (!ul || ul.hidden) {
            const path = encodedPathOf(treeSelectedEl);
            if (path !== null) {
              if (treeSelectedEl.getAttribute('data-cb-branch-state') === 'error') {
                treeBranchCache.delete(path);
              }
              treeExpandIntent.set(path, true);
              setTreeNodeExpanded(treeSelectedEl, true, true);
              loadTreeBranch(treeSelectedEl, path, null);
            }
          }
          event.preventDefault();
        }
        break;
      case 'ArrowLeft': {
        if (treeSelectedEl) {
          const ul = treeSelectedEl.querySelector(':scope > ul.cb-tree-children');
          if (isDirNode(treeSelectedEl) && ul && !ul.hidden) {
            setTreeNodeExpanded(treeSelectedEl, false, true);
          } else {
            const parentUl = treeSelectedEl.parentElement;
            const parentLi = parentUl ? parentUl.closest('li.cb-tree-node') : null;
            if (parentLi) selectTreeNode(parentLi);
          }
          event.preventDefault();
        }
        break;
      }
      case 'Enter':
        if (treeSelectedEl) {
          navigateTreeNode(treeSelectedEl);
          event.preventDefault();
        }
        break;
      case 'Backspace': {
        const current = currentPathFromState();
        if (current !== null && current !== '') {
          const parent = parentEncodedPath(current);
          if (parent !== null) {
            paneNavigate({ kind: 'directory', path: parent, history: 'push' });
          }
        }
        event.preventDefault();
        break;
      }
      default:
        break;
    }
  }

  function onKeydown(event) {
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    // v 键: 浏览视图内循环 list→grid→tree→list; 输入框/可编辑区/修饰键不触发.
    if (event.key === 'v' && !isEditableTarget(event.target) && isBrowseView()) {
      event.preventDefault();
      switchToView(nextBrowseView(state.view), { user: true });
      return;
    }
    if (state.view === 'tree' && treeShell) {
      onTreeKeydown(event);
      return;
    }
    // 面板打开时关闭; handled 时在返回前恰好一次 preventDefault, 未处理不取消;
    // 其余快捷键在可编辑元素内仍被忽略。
    if (event.key === 'Escape') {
      const input = topbarSearchInput();
      const action = escapeAction(
        document.activeElement === input,
        panel() ? !panel().hidden : false
      );
      if (action === null) return; // 未处理: 不取消默认行为
      event.preventDefault(); // handled: 恰好一次, 先于任何动作
      if (action === 'blur-search') {
        input.blur();
      } else if (action === 'close-panel') {
        closePanel();
      }
      return;
    }
    if (isEditableTarget(event.target)) return;
    // Enter 在聚焦的链接/按钮/summary 上必须走原生激活 (文件名、排序与工具栏的
    // 键盘可达性契约), 全局 Enter 只服务于无交互焦点时的选中行打开.
    if (event.key === 'Enter' && event.target.closest('a, button, summary')) return;
    switch (event.key) {
      case 'j':
      case 'ArrowDown':
        moveSelection(1);
        event.preventDefault();
        break;
      case 'k':
      case 'ArrowUp':
        moveSelection(-1);
        event.preventDefault();
        break;
      case 'Enter': {
        const target = selectedTarget();
        if (target !== null) location.href = browseActionUrl(target, 'full').href;
        event.preventDefault();
        break;
      }
      case 'o': {
        const target = selectedTarget();
        if (target !== null) {
          window.open(browseActionUrl(target, 'full').href, '_blank', 'noopener');
        }
        event.preventDefault();
        break;
      }
      case 'p':
        togglePreview();
        event.preventDefault();
        break;
      case 'Backspace': {
        const parent = $('[data-cb-parent]');
        if (parent) location.href = parent.getAttribute('href');
        event.preventDefault();
        break;
      }
      case '/': {
        if (!focusSearchInput()) return; // 无可用搜索入口: 不取消默认行为
        event.preventDefault();
        break;
      }
      default:
        break;
    }
  }

  /* ---------- 指针/文档事件 ---------- */

  function onDocumentClick(event) {
    const target = event.target;
    // 独立阅读导航: 文件树 / 复制链接 / 所在目录. 动作按钮在客户端启用前保持
    // 服务端 hidden (未启用即 no-op); 所在目录是真实链接, 只拦截主键无修饰点击.
    const treeAction = target.closest('[data-cb-reading-tree]');
    if (treeAction) {
      event.preventDefault();
      if (!treeAction.hidden) enterReadingTree();
      return;
    }
    const copyAction = target.closest('[data-cb-reading-copy]');
    if (copyAction) {
      event.preventDefault();
      if (!copyAction.hidden) {
        // 反馈区 (状态/手动选择字段) 在导航栏内, 不能藏在窄屏"更多"弹层之下.
        const panel = copyAction.closest('details');
        if (panel) panel.open = false;
        copyReadingLink();
      }
      return;
    }
    const parentAction = target.closest('[data-cb-reading-parent]');
    if (parentAction) {
      if (
        event.defaultPrevented ||
        event.button !== 0 ||
        event.metaKey ||
        event.ctrlKey ||
        event.shiftKey ||
        event.altKey
      ) {
        return; // 修饰点击/中键: 保留原生链接语义 (新 tab/新窗口)
      }
      event.preventDefault();
      navigateReadingParent();
      return;
    }
    // 顶部工具区: 视图组 / 隐藏开关 / 排序 (同一状态流程, 无第二份偏好)
    const viewBtn = target.closest('[data-cb-view]');
    if (viewBtn) {
      event.preventDefault();
      switchToView(viewBtn.getAttribute('data-cb-view'), { user: true });
      return;
    }
    const hiddenBtn = target.closest('[data-cb-hidden-toggle]');
    if (hiddenBtn) {
      event.preventDefault();
      if (isBrowseView()) applyRulesChange({ hidden: !state.rules.hidden });
      return;
    }
    const sortLink = target.closest('a[data-cb-sort]');
    if (sortLink) {
      event.preventDefault();
      const value = sortLink.getAttribute('data-cb-sort');
      if (isValidSort(value) && isBrowseView()) applyRulesChange({ sort: value });
      const details = sortLink.closest('details');
      if (details) details.open = false;
      return;
    }
    const actionBtn = target.closest(ACTION_SELECTOR);
    if (actionBtn) {
      handleAction(event, actionBtn);
      return;
    }
    // 树节点: 单击打开 — 目录展开/收起, 文件 SPA 导航; 双击连击 (detail > 1)
    // 不产生第二次动作.
    const treeRow = target.closest('.cb-tree-row');
    if (treeRow) {
      if (event.detail > 1) return;
      const li = treeRow.closest('li.cb-tree-node');
      if (!li) return;
      focusTreeRootUl(); // 先落焦点: active-descendant 公告 + 键盘可立即接管
      selectTreeNode(li);
      if (isDirNode(li)) toggleTreeNode(li); else navigateTreeNode(li);
      closeTreeDrawer(); return; // 窄屏抽屉: 选择条目后收起 (宽屏 no-op)
    }
    // 内容区/面板内目录迷你列表: 拦截可操作 anchor.
    const miniLink = target.closest('.cb-dir-mini a[data-native-path-encoded]');
    if (miniLink) {
      if (event.detail > 1) return;
      event.preventDefault();
      const path = encodedPathOf(miniLink);
      if (path === null || path === '') return;
      const li = miniLink.closest('li');
      const kind = li ? li.dataset.kind : null;
      if (kind === 'file' && isPdfFile(path, 'file')) {
        window.open(browseActionUrl(path, 'full').href, '_blank', 'noopener');
        return;
      }
      if (pane && pane.contains(miniLink)) {
        paneNavigate({
          kind: kind === 'dir' ? 'directory' : 'reading',
          path: path,
          history: 'push',
        });
        return;
      }
      if (panelContent() && panelContent().contains(miniLink)) {
        openPreview(path, kind);
        return;
      }
      location.href = browseActionUrl(path, 'full').href;
      return;
    }
    // 内容区搜索片段结果行: 打开条目 = 离开当前搜索上下文 (实际导航).
    const searchRow = target.closest('.cb-search-fragment [data-browser-entry]');
    if (searchRow) {
      if (event.detail > 1) return;
      const path = encodedPathOf(searchRow);
      if (path === null || path === '') return;
      event.preventDefault();
      const kind = entryKindOf(searchRow);
      if (kind === 'file' && isPdfFile(path, 'file')) {
        window.open(browseActionUrl(path, 'full').href, '_blank', 'noopener');
        return;
      }
      if (pane && pane.contains(searchRow)) {
        paneNavigate({
          kind: kind === 'dir' ? 'directory' : 'reading',
          path: path,
          history: 'push',
        });
        return;
      }
      location.href = browseActionUrl(path, 'full').href;
      return;
    }
    // 非链接点击: 选中行 (文件名 anchor 保持原生行为)
    const entry = target.closest(ENTRY_SELECTOR);
    if (entry && !target.closest('a')) {
      selectEntry(entry);
    }
  }

  function onDocumentDblclick(event) {
    // 双击落在任何 action/button/control/link 上绝不触发行 Full 导航.
    if (
      event.target.closest('a, button, input, select, textarea, [data-cb-action]')
    ) {
      return;
    }
    // 树节点双击: 单击已打开, 双击必须无第二次动作.
    if (event.target.closest('.cb-tree-row')) {
      event.preventDefault();
      return;
    }
    const entry = event.target.closest(ENTRY_SELECTOR);
    if (entry) {
      const target = encodedPathOf(entry);
      if (target !== null && target !== '') location.href = browseActionUrl(target, 'full').href;
    }
  }

  /* ---------- 历史/生命周期 ---------- */

  // 活内存状态是否已对应该快照目标 (BFCache 恢复/锚点导航等幂等场景).
  function snapshotMatchesState(snapshot) {
    if (snapshot.surface !== state.kind || snapshot.view !== state.view) return false;
    if (snapshot.surface === 'search') return (snapshot.q || '') === (state.q || '');
    return (
      canonicalTransportKey(snapshot.path) === canonicalTransportKey(state.path)
    );
  }

  // 树壳恢复的展开意图:
  // - 冷访问 (无快照): 既有约定 — 展开当前内容路径;
  // - 快照携带的定位选中落在当前内容目标之内 (自身或后代) 时是"定位"语义: 展开
  //   当前路径链, 让定位目标可见并选中 (父目录定位文件 / 阅读目标进树);
  // - 其余快照恢复只恢复既有展开 — 用户显式折叠的祖先不因内容定位重新展开.
  function treeRestoreReveal() {
    if (!bootSnapshot) return true;
    const selected = state.selected;
    if (selected === null || !isValidEncodedPath(selected) || selected === '') return false;
    const current = currentPathFromState();
    if (current === null) return false;
    if (selected === current) return true;
    return current === '' || selected.indexOf(current + '/') === 0;
  }

  // back/forward: managed 快照优先按项恢复 (不重读偏好). 跨文档 BFCache 恢复与
  // 同文档 SPA 项都会触发 popstate: 活 DOM 已匹配目标时绝不重载/重取; 只有
  // 当前文档确实无法代表目标表面时才整页恢复.
  function onPopState() {
    if (!initialized) return;
    const snapshot = readSnapshot();
    if (snapshot && snapshotMatchesLocation(snapshot)) {
      const alreadyCurrent = snapshotMatchesState(snapshot);
      applySnapshot(snapshot);
      syncControlState();
      // 活 DOM 已对应该项 (跨文档 BFCache 恢复 / 锚点导航): 零 DOM 变更、零重取
      // (树壳缺失时才重建, 属防御).
      if (alreadyCurrent) {
        if (state.view === 'tree' && !treeShell) ensureTreeShell(treeRestoreReveal());
        return;
      }
      if (state.view === 'tree') {
        ensureTreeShell(treeRestoreReveal()); // 同文档 SPA 项: 树壳保持, 按快照目标重同步
        return;
      }
      location.reload(); // 目标表面非树壳且活 DOM 不匹配: 整页加载按快照恢复
      return;
    }
    if (isSearchLocation()) {
      const q = urlParams().get('q') || '';
      if (state.kind !== 'search' || q !== (state.q || '')) location.reload();
      return;
    }
    const urlPath = locationEncodedPath();
    if (urlPath === null) {
      location.reload();
      return;
    }
    if (urlPath !== state.path) location.reload();
    // hash-only 导航: 保留现状 (既有锚点规则: 不重取).
  }

  // BFCache 恢复: 按当前历史项快照幂等重放 (不重读偏好, 已安装内容不重取).
  function onPageShow(event) {
    if (!initialized || !event.persisted) return;
    const snapshot = readSnapshot();
    if (!snapshot || !snapshotMatchesLocation(snapshot)) return;
    const alreadyCurrent = snapshotMatchesState(snapshot);
    applySnapshot(snapshot);
    syncControlState();
    // BFCache 恢复: DOM 已冻结为该项状态 → 幂等重放, 不重取、不重读偏好.
    if (alreadyCurrent) {
      if (state.view === 'tree' && !treeShell) ensureTreeShell(treeRestoreReveal());
      return;
    }
    if (state.view === 'tree') {
      ensureTreeShell(treeRestoreReveal());
      return;
    }
    if (state.view === 'list' || state.view === 'grid') {
      applyViewContainers();
      applyEntrySelection({ silent: true });
    }
  }

  // 离开文档 (reload/导航/BFCache) 前把真实阅读位置写回当前历史项: 恢复时不会
  // 拿到陈旧滚动值 (只对阅读表面生效, 不触碰其它表面与浏览视图). 树壳文档的
  // window 不滚动 (body overflow:hidden) — 回退到阅读项 (popstate 后的 reload)
  // 时绝不用树壳的 0 覆盖已记录的阅读位置.
  function onPageHide() {
    if (!initialized || !isReadingSurface() || treeShell) return;
    replaceCurrentEntry();
  }

  /* ---------- 启动 ---------- */

  // 共享工具区边界变量 (--cb-topbar-bottom): 普通流中的工具区底部, 预览面板与
  // 窄屏抽屉/遮罩从其下方开始, 跟随窄屏换行与克隆后的实际高度.
  function installTopbarBounds(topbar) {
    const update = function () {
      document.body.style.setProperty(
        '--cb-topbar-bottom',
        topbar.offsetTop + topbar.offsetHeight + 'px'
      );
    };
    new ResizeObserver(update).observe(topbar);
    update();
  }

  function installListeners() {
    document.addEventListener('click', onDocumentClick);
    document.addEventListener('dblclick', onDocumentDblclick);
    document.addEventListener('keydown', onKeydown);
    window.addEventListener('popstate', onPopState);
    window.addEventListener('pageshow', onPageShow);
    window.addEventListener('pagehide', onPageHide);
    if (window.matchMedia) {
      window
        .matchMedia('(prefers-color-scheme: dark)')
        .addEventListener('change', rethemeMermaids);
    }
    installTocSpyMql();
    const topbar = $('header.cb-topbar');
    if (topbar) installTopbarBounds(topbar);
  }

  function init() {
    if (initialized) return;
    const tokenMeta = document.querySelector('meta[name="cb-native-open-token"]');
    nativeToken = tokenMeta ? tokenMeta.getAttribute('content') : null;
    // 内容特性 (TOC/公式图表) 与浏览增强独立: 无显式页面语义时只保留内容特性.
    initFullPageTocSpy();
    renderMermaidIn(document);
    const surface = bodySurface();
    if (surface === null) {
      initialized = true;
      installListeners();
      return;
    }
    const pagePath = encodedPathOf(document.body);
    if (pagePath === null) {
      // 完整页缺少规范 identity 时 fail-closed, 不从 location 或可见标题猜 action.
      initialized = true;
      installListeners();
      return;
    }
    // 独立阅读导航: 只有合法 identity 才启用服务端 hidden 的依赖 JS 动作
    // (上一分支 fail-closed 时不触及任何控件).
    if (surface === 'reading') installReadingNav();
    // 1) 有效状态: managed 快照优先 (reload/popstate/BFCache 按该项恢复);
    //    否则显式 URL > 保存偏好 > 默认.
    const snapshot = readSnapshot();
    const useSnapshot = !!(snapshot && snapshotMatchesLocation(snapshot));
    bootSnapshot = useSnapshot;
    if (useSnapshot) {
      applySnapshot(snapshot);
      // 独立阅读位置: 只消费快照记录的窗口滚动 (进入树前的真实位置), 不依赖
      // 浏览器对锚点或历史项的默认滚动恢复.
      if (snapshot.surface === 'reading' && snapshot.view === null) {
        restoreReadingScroll(snapshot.scroll);
      }
    } else {
      const explicit = explicitUrlRules();
      state.kind = surface;
      state.path = pagePath;
      state.q = surface === 'search' ? urlParams().get('q') || '' : null;
      state.rules.sort = explicit.sort !== null ? explicit.sort : loadSortPref();
      state.rules.hidden = explicit.hidden !== null ? explicit.hidden : loadHiddenPref();
      state.view = surface === 'reading' ? null : loadViewPref();
      state.opened = null;
      state.openedKind = null;
      state.selected = null;
    }
    // 2) 服务器渲染内容与有效规则不一致 (URL 未显式携带、偏好覆盖默认):
    //    浏览布局整页重取一次对齐内容 (同位置 replace, 不写偏好, 不新增历史).
    const bodySort = document.body.dataset.cbSort;
    const bodyHidden = document.body.dataset.cbHidden;
    const rulesMismatch =
      (isValidSort(bodySort) && bodySort !== state.rules.sort) ||
      (isValidHidden(bodyHidden) && (bodyHidden === '1') !== state.rules.hidden);
    if (rulesMismatch && (state.view === 'list' || state.view === 'grid')) {
      initialized = true;
      replaceCurrentEntry(currentEntryUrlWithRules());
      location.reload();
      return;
    }
    // 3) 初始化规范化 (先于消费快照之后的 DOM 应用): 浏览页 URL 显式携带有效
    //    规则与快照; 独立阅读页 URL 不变只补写快照.
    if (surface === 'reading') {
      if (!useSnapshot || rulesMismatch) replaceCurrentEntry();
    } else {
      const params = urlParams();
      const canonical =
        params.get('sort') === state.rules.sort &&
        params.get('hidden') === (state.rules.hidden ? '1' : '0') &&
        (surface !== 'search' || params.get('q') === (state.q || ''));
      if (!canonical || !useSnapshot || rulesMismatch) {
        replaceCurrentEntry(currentEntryUrlWithRules());
      }
    }
    // 4) 应用视图与控件.
    if (state.view === 'tree') {
      ensureTreeShell(treeRestoreReveal());
    } else if (state.view === 'list' || state.view === 'grid') {
      syncControlState();
      applyViewContainers();
      applyEntrySelection({ silent: true });
      restoreOpenedPanel();
    } else {
      syncControlState();
    }
    initialized = true;
    installListeners();
  }

  // 恢复已打开条目 (reload/popstate/BFCache): 不打开回退项; PDF 不自动新 tab.
  function restoreOpenedPanel() {
    const opened = state.opened;
    if (opened === null || opened === '' || !isValidEncodedPath(opened)) return;
    const entry = findEntryByIdentity(opened);
    const domKind = entry ? entryKindOf(entry) : null;
    const kind = domKind !== null ? domKind : state.openedKind === 'directory' ? 'dir' : 'file';
    // 目录名 *.pdf 不是 PDF: kind 双条件保证 (DOM kind=dir 从不匹配).
    if (isPdfFile(opened, kind)) return;
    openPreview(opened, kind, { silent: true });
  }

  if (typeof document !== 'undefined') {
    if (document.readyState === 'loading') {
      document.addEventListener('DOMContentLoaded', init);
    } else {
      init();
    }
  }

  /* ---------- 测试钩子 (仅 CommonJS 环境; 浏览器无 module, 不产生全局) ---------- */
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = {
      isValidEncodedPath: isValidEncodedPath,
      actionUrl: actionUrl,
      nativeOpenParams: nativeOpenParams,
      escapeAction: escapeAction,
      panelTargetForSelection: panelTargetForSelection,
      createGenerationGuard: createGenerationGuard,
      canonicalTransportKey: canonicalTransportKey,
      canStartTreeResize: canStartTreeResize,
      ownsTreeResize: ownsTreeResize,
      collapseFocusTarget: collapseFocusTarget,
      treeKeyScope: treeKeyScope,
      isPdfFile: isPdfFile,
      isValidSort: isValidSort,
      isValidHidden: isValidHidden,
      nextBrowseView: nextBrowseView,
      parentEncodedPath: parentEncodedPath,
      isHiddenName: isHiddenName,
      isHiddenEncodedPath: isHiddenEncodedPath,
      nearestVisibleIndex: nearestVisibleIndex,
      treeLoadDecision: treeLoadDecision,
      isValidSnapshot: isValidSnapshot,
    };
  }
})();
