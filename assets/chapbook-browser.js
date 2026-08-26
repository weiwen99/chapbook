/* chapbook-browser.js — 目录页/搜索页渐进增强 (vanilla IIFE, 无依赖/构建).
 *
 * 安全契约 (docs/2026-08-12-design-readonly-file-browser.org, browser action path
 * contract): `element.dataset.nativePathEncoded` 是行/面板唯一的 action target。
 * - URL actions (当前 tab / 新 tab / 预览 / 目录下钻 / 下载) 全程不 decode
 *   encodedPath: 统一经 actionUrl() 构造 URL, 模式只经 URL.searchParams 添加;
 * - native-open 独占唯一一次 decodeURIComponent, 随后交给 URLSearchParams 做
 *   application/x-www-form-urlencoded 编码 (含 token);
 * - 缺失/非法 target 一律 fail-closed: 清空面板 target, 禁用全部 action,
 *   不发送请求, 不复用旧 target。
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


  /* ---------- DOM 状态与渲染 ---------- */

  const ENTRY_SELECTOR = '[data-browser-entry]';
  const ACTION_SELECTOR = '[data-cb-action]';
  const PANEL_SELECTOR = '#cb-preview';
  // 需要 panel target 才能启用的工具栏按钮 (close 永远可用)
  const TARGET_ACTIONS = { full: true, new: true, download: true, native: true };
  const VIEW_KEY = 'cb-view';

  let selectedIndex = -1;
  let panelTarget = null; // 合法 encodedPath 或 null (fail-closed)
  let nativeToken = null;
  const previewGuard = createGenerationGuard(); // 并发预览 generation 守卫

  let view = 'list';
  try {
    view = localStorage.getItem(VIEW_KEY) === 'grid' ? 'grid' : 'list';
  } catch (err) {
    view = 'list';
  }

  function $(sel, root) {
    return (root || document).querySelector(sel);
  }

  function $$(sel, root) {
    return Array.from((root || document).querySelectorAll(sel));
  }

  function searchInput() {
    return $('#cb-search-q');
  }

  function viewList() {
    return $('#cb-view-list');
  }

  function viewGrid() {
    return $('#cb-view-grid');
  }

  function panel() {
    return $(PANEL_SELECTOR);
  }

  function panelContent() {
    return $('#cb-preview-content');
  }

  function panelTitle() {
    return $('#cb-preview-title');
  }

  function currentView() {
    return view;
  }

  // 视图切换: 列表/网格渲染同一批 entries (同序), selectedIndex 语义不变;
  // 切换后必须把 cb-selected 重新应用到新视图的对应条目, 否则键盘状态
  // 与可见选中行分叉.
  function applyView() {
    const grid = view === 'grid';
    if (viewList()) viewList().hidden = grid;
    if (viewGrid()) viewGrid().hidden = !grid;
    $$('[data-cb-view]').forEach(function (btn) {
      btn.setAttribute(
        'aria-pressed',
        btn.getAttribute('data-cb-view') === view ? 'true' : 'false'
      );
    });
    $$(ENTRY_SELECTOR).forEach(function (el) {
      el.classList.remove('cb-selected');
    });
    const entries = visibleEntries();
    if (selectedIndex >= 0 && selectedIndex < entries.length) {
      entries[selectedIndex].classList.add('cb-selected');
    }
    try {
      localStorage.setItem(VIEW_KEY, view);
    } catch (err) {
      /* 隐私模式等场景忽略持久化失败 */
    }
  }

  // 行/面板唯一 action target: 属性缺失或非法 → null (禁用全部 action)。
  function encodedPathOf(el) {
    if (!el || !el.dataset) return null;
    const value = el.dataset.nativePathEncoded;
    if (value === undefined || !isValidEncodedPath(value)) return null;
    return value;
  }

  function visibleEntries() {
    const host = view === 'grid' ? viewGrid() : viewList();
    return host ? $$(ENTRY_SELECTOR, host) : [];
  }

  function selectIndex(index) {
    const entries = visibleEntries();
    if (entries.length === 0) {
      selectedIndex = -1;
      return;
    }
    const clamped = Math.max(0, Math.min(index, entries.length - 1));
    if (selectedIndex >= 0 && selectedIndex < entries.length) {
      entries[selectedIndex].classList.remove('cb-selected');
    }
    selectedIndex = clamped;
    entries[selectedIndex].classList.add('cb-selected');
    entries[selectedIndex].scrollIntoView({ block: 'nearest' });
    // P2#4: 选中非 actionable 行立即清空旧 panel target 并禁用 actions,
    // 绝不复用 stale target。
    panelTarget = panelTargetForSelection(
      encodedPathOf(entries[selectedIndex]),
      panelTarget
    );
    if (panelTarget === null) setActionsEnabled(false);
  }

  function selectEntry(entry) {
    const idx = visibleEntries().indexOf(entry);
    if (idx >= 0) selectIndex(idx);
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

  // fail-closed: 清空面板 target 并禁用依赖 target 的工具栏按钮。
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
    if (!isValidEncodedPath(encodedPath)) {
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
    setPanelTitleText('');
    if (panelContent()) panelContent().textContent = '';
  }

  function showPanelError(message) {
    setPanelTitleText(message);
    if (panelContent()) panelContent().textContent = '';
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

  // fragment wrapper 恒为响应的根元素; 面板 identity 只从根 wrapper 自身的
  // data 读取, 绝不从后代元素 (迷你列表条目等) 反推.
  function findFragmentWrapper(host) {
    return host.firstElementChild;
  }

  // 预览: 先清空旧 target/actions 再 fetch; 要求 ok + text/html 才 innerHTML,
  // 错误用 textContent; target 只从返回 wrapper 的合法 encoded data 安装。
  // P2#5: generation 守卫 — 新请求/面板关闭使在途请求过期, 旧响应/错误
  // 不得 overwrite/install 任何状态。
  function openPreview(encodedPath) {
    if (!isValidEncodedPath(encodedPath)) return; // fail-closed
    const gen = previewGuard.begin(); // 新请求作废所有在途请求
    clearPanelTarget();
    openPanel();
    const url = actionUrl(encodedPath, 'fragment');
    fetch(url, { headers: { Accept: 'text/html' } })
      .then(function (res) {
        if (!previewGuard.isCurrent(gen)) return; // 过期响应: 丢弃
        return readFragmentResponse(res);
      })
      .then(function (html) {
        if (!previewGuard.isCurrent(gen)) return; // 过期响应: 丢弃
        const host = panelContent();
        host.textContent = '';
        host.innerHTML = html;
        const wrapper = findFragmentWrapper(host);
        if (!wrapper) {
          clearPanelTarget();
          setPanelTitleText('');
          return;
        }
        installPanelTarget(wrapper.dataset.nativePathEncoded);
        // 目录 fragment: 下载对目录无意义 (服务端忽略目录 download 模式),
        // 单独禁用; 其余 target 相关 action 保持可用.
        if (panelTarget !== null) {
          setDownloadEnabled(!wrapper.classList.contains('cb-dir-fragment'));
        }
        installPanelTitleMarkup(wrapper.querySelector('.cb-frag-title'));
      })
      .catch(function (err) {
        if (!previewGuard.isCurrent(gen)) return; // 过期错误: 丢弃
        clearPanelTarget();
        showPanelError('无法预览：' + err.message);
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
    // 树模式无 native-open (Q17): native action 一律 no-op
    if (action === 'native' && treeMode) return;
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
      openPreview(target);
    } else if (action === 'full') {
      location.href = actionUrl(target, 'full').href;
    } else if (action === 'new') {
      window.open(actionUrl(target, 'full').href, '_blank', 'noopener');
    } else if (action === 'download') {
      location.href = actionUrl(target, 'download').href;
    } else if (action === 'native') {
      openNative(target);
    }
  }

  function onDocumentClick(event) {
    const actionBtn = event.target.closest(ACTION_SELECTOR);
    if (actionBtn) {
      handleAction(event, actionBtn);
      return;
    }
    // 树节点: 单击选中; 目录同时展开/收起 (不导航)
    const treeRow = event.target.closest('.cb-tree-row');
    if (treeRow) {
      const li = treeRow.closest('li.cb-tree-node');
      if (li) {
        selectTreeNode(li);
        if (isDirNode(li)) toggleTreeNode(li);
        closeTreeDrawer(); // 窄屏抽屉: 选择条目后收起 (宽屏 no-op)
      }
      return;
    }
    // 面板/内容区内目录迷你列表: 拦截可操作 anchor — 树模式 SPA 下钻,
    // 目录模式保持面板内下钻
    const miniLink = event.target.closest(
      '.cb-dir-mini a[data-native-path-encoded]'
    );
    if (miniLink) {
      event.preventDefault();
      const target = encodedPathOf(miniLink);
      if (treeMode) {
        if (target !== null) paneNavigate(target);
      } else {
        openPreview(target);
      }
      return;
    }
    // 非链接点击: 选中行 (文件名 anchor 保持原生行为)
    const entry = event.target.closest(ENTRY_SELECTOR);
    if (entry && !event.target.closest('a')) {
      selectEntry(entry);
    }
  }

  function onDocumentDblclick(event) {
    // P2#3: 双击落在任何 action/button/control/link 上绝不触发行 Full 导航
    // (链接/按钮/表单控件走各自原生行为; action 按钮双击仍只触发两次单击动作)。
    if (
      event.target.closest(
        'a, button, input, select, textarea, [data-cb-action]'
      )
    ) {
      return;
    }
    // 树节点双击: 整页导航 (与 Enter 一致)
    const treeRow = event.target.closest('.cb-tree-row');
    if (treeRow) {
      const li = treeRow.closest('li.cb-tree-node');
      if (li) navigateTreeNode(li);
      return;
    }
    const entry = event.target.closest(ENTRY_SELECTOR);
    if (entry) {
      const target = encodedPathOf(entry);
      if (target !== null) location.href = actionUrl(target, 'full').href;
    }
  }

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

  function togglePreview() {
    const target = selectedTarget();
    if (target === null) return;
    const el = panel();
    if (el && !el.hidden && panelTarget === target) {
      closePanel();
      return;
    }
    openPreview(target);
  }

  function onKeydown(event) {
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    if (treeMode) {
      onTreeKeydown(event);
      return;
    }
    // 面板打开时关闭; handled 时在返回前恰好一次 preventDefault, 未处理不取消;
    // 其余快捷键在可编辑元素内仍被忽略。
    if (event.key === 'Escape') {
      const input = searchInput();
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
    // Enter 在聚焦的链接/按钮上必须走原生激活 (文件名 anchor、工具栏按钮的
    // 键盘可达性契约), 全局 Enter 只服务于无交互焦点时的选中行打开.
    if (event.key === 'Enter' && event.target.closest('a, button')) return;
    const key = event.key;
    switch (key) {
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
        if (target !== null) location.href = actionUrl(target, 'full').href;
        event.preventDefault();
        break;
      }
      case 'o': {
        const target = selectedTarget();
        if (target !== null) {
          window.open(actionUrl(target, 'full').href, '_blank', 'noopener');
        }
        event.preventDefault();
        break;
      }
      case 'p':
        togglePreview();
        event.preventDefault();
        break;
      case 'v':
        view = view === 'grid' ? 'list' : 'grid';
        applyView();
        event.preventDefault();
        break;
      case 'Backspace': {
        const parent = $('[data-cb-parent]');
        if (parent) location.href = parent.getAttribute('href');
        event.preventDefault();
        break;
      }
      case '/': {
        const input = searchInput();
        if (input) input.focus();
        event.preventDefault();
        break;
      }
      default:
        break;
    }
  }

  /* ---------- 树模式 (编辑器式浏览外壳) ---------- */

  // 模式/树状态持久化键. 默认目录模式 (无键即目录模式).
  const MODE_KEY = 'cb-mode';
  const TREE_EXPANDED_KEY = 'cb-tree-expanded';
  const TREE_SELECTED_KEY = 'cb-tree-selected';
  const TREE_WIDTH_KEY = 'cb-tree-width'; // 树宽持久化键 (px)
  const TREE_DEFAULT_WIDTH = 238;
  const TREE_MIN_WIDTH = 160;
  const TREE_MAX_WIDTH = 480;
  const TREE_DRAWER_MQL = '(width < 64rem)'; // 窄屏抽屉断点 (与 CSS 媒体查询同源)

  let treeMode = false;
  let treeNav = null; // nav.cb-tree 外壳
  let treeRootUl = null; // 根 <ul>
  // 已加载子项缓存: encoded dir path (无前导斜杠) -> 子节点 <li> 数组
  const treeCache = new Map();
  let treeSelectedEl = null;
  // 树壳生命周期 generation: 退出树模式递增使全部在途响应失效 (镜像 previewGuard)
  let treeGen = 0;
  // 窄屏抽屉 (<64rem, off-canvas transform + 遮罩): 随树壳创建/销毁.
  // 开合态挂 body.cb-drawer-open (宽屏无对应 CSS, 恒 no-op).
  let treeScrim = null; // 遮罩: 点击收起抽屉
  let treeOpenBtn = null; // 唤起按钮 (仅窄屏可见)
  let drawerMql = null; // 窄屏 media query (跨回宽屏时清理打开态)
  let drawerMqlHandler = null;
  // 树宽 (宽屏列 1): resizer 常驻调整; 窄屏抽屉内 resizer 隐藏, 宽度值不生效.
  // treePreferredWidth 是持久化偏好: 仅硬边界 [160,480] 截断, 绝不被窄屏 50vw
  // 上限改写; treeWidth 是当前生效宽 (宽屏 = 偏好按视口约束, 窄屏抽屉忽略).
  let treePreferredWidth = TREE_DEFAULT_WIDTH;
  let treeWidth = TREE_DEFAULT_WIDTH;
  let treeResizer = null; // 列 2 调整器元素 (随树壳创建/销毁)
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

  let treeExpandedPaths = loadJsonArray(TREE_EXPANDED_KEY, []);

  // 搜索页 (page_shell 带 data-cb-parent) 不挂树壳、无切换按钮.
  function isSearchPage() {
    return !!document.querySelector('main[data-cb-parent]');
  }

  // 可挂树壳的完整页: 目录页 (.cb-browser-page 且非搜索页) 与文档/代码/Office 页.
  function isTreeCapablePage() {
    return (
      !!document.querySelector('.cb-doc-page') ||
      (!!document.querySelector('.cb-browser-page') && !isSearchPage())
    );
  }

  function treePathFromHref(href) {
    // '/a%20b/c.md' -> 'a%20b/c.md'; 必须过合法 encodedPath 校验 (fail-closed)
    if (typeof href !== 'string' || href[0] !== '/') return null;
    const rel = href.slice(1);
    return isValidEncodedPath(rel) ? rel : null;
  }

  function currentPathFromLocation() {
    const pathname = location.pathname;
    if (pathname === '/') return '';
    // 目录深链可能带尾斜杠 (/subdir/): 剥掉后校验 (canonical 形式无尾斜杠)
    const rel = pathname.slice(1).replace(/\/+$/, '');
    return isValidEncodedPath(rel) ? rel : null;
  }

  function treeSearchForm() {
    const form = document.createElement('form');
    form.className = 'cb-tree-search';
    form.action = '/__/search';
    form.method = 'get';
    form.setAttribute('role', 'search');
    const input = document.createElement('input');
    input.type = 'search';
    input.name = 'q';
    input.id = 'cb-tree-search-q';
    input.placeholder = '搜索文件';
    input.setAttribute('aria-label', '搜索文件');
    input.autocomplete = 'off';
    form.append(input);
    return form;
  }

  // JSON -> DOM 节点: 文件名只经 createElement + textContent (外层 bdi),
  // 绝不插值进 innerHTML/HTML 模板. href 过合法 encodedPath 校验后进 dataset.
  function buildTreeNode(entry) {
    const li = document.createElement('li');
    li.className = 'cb-tree-node';
    const name = typeof entry.name === 'string' ? entry.name : '';
    const isDir = entry.dir === true;
    const href = typeof entry.href === 'string' ? entry.href : null;
    const relPath = treePathFromHref(href);
    // 可展开 = 目录 && 服务端 expandable && 有合法 href (环/非 UTF-8 目录恒不可展开)
    const expandable = isDir && entry.expandable === true && relPath !== null;
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
    row.append(twist);

    const icon = document.createElement('span');
    icon.className = 'cb-tree-icon';
    icon.textContent = isDir ? '📁' : '📄';
    row.append(icon);

    if (!isDir) {
      const badge = document.createElement('span');
      badge.className = 'cb-tree-badge';
      const dot = name.lastIndexOf('.');
      const ext = dot > 0 ? name.slice(dot + 1) : '';
      badge.textContent = ext ? ext.slice(0, 5).toUpperCase() : 'FILE';
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
    return path === null ? null : actionUrl(path, 'full');
  }

  function setTwist(li, expanded) {
    const twist = li.querySelector(':scope > .cb-tree-row .cb-tree-twist');
    if (twist) twist.textContent = expanded ? '▾' : '▸';
  }

  function setTreeNodeExpanded(li, expanded) {
    const ul = li.querySelector(':scope > ul.cb-tree-children');
    if (ul) ul.hidden = !expanded;
    if (isExpandableDir(li)) setTwist(li, expanded);
    const path = encodedPathOf(li);
    if (path === null) return;
    const idx = treeExpandedPaths.indexOf(path);
    if (expanded && idx < 0) {
      treeExpandedPaths.push(path);
    } else if (!expanded && idx >= 0) {
      treeExpandedPaths.splice(idx, 1);
    }
    saveJsonArray(TREE_EXPANDED_KEY, treeExpandedPaths);
  }

  // 拉取目录子项 JSON: path 为空串时命中显式根路由 /__/tree (绝不拼尾斜杠).
  function fetchTreeEntries(path) {
    const url = path === '' ? '/__/tree' : '/__/tree/' + path;
    return fetch(url, { headers: { Accept: 'application/json' } })
      .then(function (res) {
        if (!res.ok) throw new Error('HTTP ' + res.status);
        return res.json();
      })
      .then(function (data) {
        return data && Array.isArray(data.entries) ? data.entries : [];
      });
  }

  // 懒加载目录子项; 失败静默 (网络/服务端错误不留半展开态).
  function loadTreeChildren(li, path) {
    const gen = treeGen;
    if (treeCache.has(path)) {
      const ul = childrenUlFor(li);
      ul.replaceChildren(...treeCache.get(path));
      setTreeNodeExpanded(li, true);
      return Promise.resolve();
    }
    return fetchTreeEntries(path)
      .then(function (entries) {
        if (gen !== treeGen) return; // 旧壳在途响应: 不得触碰新壳 DOM/缓存
        const nodes = entries.map(buildTreeNode);
        treeCache.set(path, nodes);
        const ul = childrenUlFor(li);
        ul.replaceChildren(...nodes);
        setTreeNodeExpanded(li, true);
      })
      .catch(function () {
        /* 加载失败: 不标记展开, 下次点击重试 */
      });
  }

  function toggleTreeNode(li) {
    if (!isExpandableDir(li)) return;
    const ul = li.querySelector(':scope > ul.cb-tree-children');
    const expanded = !!ul && !ul.hidden;
    if (expanded) {
      setTreeNodeExpanded(li, false);
      return;
    }
    const path = encodedPathOf(li);
    if (path === null) return;
    loadTreeChildren(li, path);
  }

  function selectTreeNode(li) {
    if (treeSelectedEl) treeSelectedEl.classList.remove('cb-selected');
    treeSelectedEl = li;
    li.classList.add('cb-selected');
    const path = encodedPathOf(li);
    try {
      if (path === null) {
        localStorage.removeItem(TREE_SELECTED_KEY);
      } else {
        localStorage.setItem(TREE_SELECTED_KEY, path);
      }
    } catch (err) {
      /* 持久化失败忽略 */
    }
  }

  function clearTreeSelection() {
    if (treeSelectedEl) treeSelectedEl.classList.remove('cb-selected');
    treeSelectedEl = null;
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

  // 恢复持久化选中路径 (visible 节点内查找; 深层未展开则不可见, 跳过).
  function restoreTreeSelection() {
    let saved = null;
    try {
      saved = localStorage.getItem(TREE_SELECTED_KEY);
    } catch (err) {
      return;
    }
    if (saved === null || !isValidEncodedPath(saved)) return;
    const node = visibleTreeNodes().find(function (li) {
      return encodedPathOf(li) === saved;
    });
    if (node) selectTreeNode(node);
  }

  function moveTreeSelection(delta) {
    const nodes = visibleTreeNodes();
    if (nodes.length === 0) return;
    const idx = nodes.indexOf(treeSelectedEl);
    const base = idx < 0 ? (delta > 0 ? -1 : 0) : idx;
    const next = Math.max(0, Math.min(base + delta, nodes.length - 1));
    selectTreeNode(nodes[next]);
    nodes[next].scrollIntoView({ block: 'nearest' });
  }

  function navigateTreeNode(li) {
    const url = nodeHref(li);
    if (url === null) return;
    const path = encodedPathOf(li);
    // 树内导航: SPA (fragment 注入 + pushState); 无 pane (异常) 时兜底整页导航.
    if (pane && path !== null) {
      paneNavigate(path);
      return;
    }
    location.href = url.href;
  }

  // 在当前层查找 href 匹配的树节点 (pathname 比较, 不 decode).
  function findTreeNode(parentUl, href) {
    return $$(':scope > li', parentUl).find(function (li) {
      return nodeHref(li) !== null && nodeHref(li).pathname === href;
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
      const children = node.querySelector(':scope > ul.cb-tree-children');
      parentUl = isDirNode(node) && children && !children.hidden ? children : null;
    });
    // 只有最深匹配节点标 current (祖先只展开, 不高亮)
    if (lastNode) lastNode.classList.add('cb-current');
    return lastNode;
  }

  // 展开并定位当前 URL 路径: 逐段匹配节点 (按 href, 不分目录/文件), 匹配即标
  // current; 可展开目录继续下钻, 环/非 UTF-8 目录或文件处终止. 解析为最深节点.
  function revealPath(segments, gen) {
    let parentUl = treeRootUl;
    let lastNode = null;
    let promise = Promise.resolve();
    segments.forEach(function (seg, i) {
      const href = '/' + segments.slice(0, i + 1).join('/');
      promise = promise.then(function () {
        // 陈旧导航的链路不得继续标记 (single-active-navigation)
        if (gen !== undefined && !paneNavGuard.isCurrent(gen)) return;
        if (!parentUl) return;
        const node = findTreeNode(parentUl, href);
        if (!node) {
          parentUl = null;
          return;
        }
        lastNode = node;
        if (isExpandableDir(node)) {
          const path = encodedPathOf(node);
          if (path === null) {
            parentUl = null;
            return;
          }
          return loadTreeChildren(node, path).then(function () {
            parentUl = node.querySelector(':scope > ul.cb-tree-children');
          });
        }
        parentUl = null;
      });
    });
    return promise.then(function () {
      // 只有最深匹配节点标 current (祖先只展开, 不高亮)
      if (gen !== undefined && !paneNavGuard.isCurrent(gen)) return lastNode;
      if (lastNode) lastNode.classList.add('cb-current');
      return lastNode;
    });
  }

  // 树宽上限: 视口一半封顶 (极窄视口下两栏仍各得其所), 且不超 480px.
  function treeWidthMax() {
    return Math.min(TREE_MAX_WIDTH, Math.floor(window.innerWidth * 0.5));
  }
  // 持久化偏好只受硬边界约束 [160,480], 绝不受窄屏 50vw 上限影响.
  function clampTreePreferred(width) {
    return Math.max(TREE_MIN_WIDTH, Math.min(TREE_MAX_WIDTH, Math.round(width)));
  }
  // 生效宽: 偏好再按当前视口约束 (宽屏恒等于偏好; 窄屏抽屉忽略该值).
  function clampTreeWidth(width) {
    return Math.max(TREE_MIN_WIDTH, Math.min(treeWidthMax(), Math.round(width)));
  }
  // 恢复持久化偏好: 仅正数合法 (0/NaN/负数 → 默认 238), 再按硬边界截断.
  function loadTreePreferredWidth() {
    let saved = 0;
    try {
      saved = Number(localStorage.getItem(TREE_WIDTH_KEY));
    } catch (err) {
      /* 持久化读取失败: 用默认值 */
    }
    return saved > 0 ? clampTreePreferred(saved) : TREE_DEFAULT_WIDTH;
  }
  // 应用树宽到 CSS 变量 (body.cb-tree-mode 列 1) 并同步 resizer ARIA;
  // persist 时按硬边界截断更新持久化偏好 (存储异常忽略, 宽度仍生效).
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
  function buildTreeShell() {
    const gen = treeGen;
    treeNav = document.createElement('nav');
    treeNav.className = 'cb-tree';
    treeNav.setAttribute('aria-label', '目录树');
    treeNav.tabIndex = -1; // 窄屏抽屉打开时程序化聚焦 (非 tab 序)
    treeNav.append(treeSearchForm());
    treeRootUl = document.createElement('ul');
    treeRootUl.className = 'cb-tree-root';
    treeNav.append(treeRootUl);
    document.body.prepend(treeNav);
    // 列 2 树宽调整器 (宽屏常驻; 窄屏抽屉内 display:none, 无行为): ARIA
    // separator + 指针拖拽 (捕获) + 键盘 ±16/Home/End 步进. 属性值随
    // treeWidth 同步 (setTreeWidth 已先于外壳就位执行/或在外壳内保持).
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
      // 未来的折叠按钮 (.cb-tree-collapse 子控件) 不触发拖拽: 先于捕获返回.
      if (event.target.closest('.cb-tree-collapse')) return;
      const startX = event.clientX;
      const startWidth = treeWidth;
      let dragWidth = startWidth; // 未截断的目标宽: 结束时按硬边界持久化
      try {
        treeResizer.setPointerCapture(event.pointerId);
      } catch (err) {
        /* 指针捕获失败: 仍按未捕获拖拽处理, 保持窗口内可调整 */
      }
      document.body.classList.add('cb-tree-resizing');
      if (event.cancelable) event.preventDefault();
      const onMove = function (moveEvent) {
        dragWidth = startWidth + moveEvent.clientX - startX;
        setTreeWidth(dragWidth, false);
        if (moveEvent.cancelable) moveEvent.preventDefault();
      };
      const onEnd = function () {
        treeResizer.removeEventListener('pointermove', onMove);
        treeResizer.removeEventListener('pointerup', onEnd);
        treeResizer.removeEventListener('pointercancel', onEnd);
        try {
          treeResizer.releasePointerCapture(event.pointerId);
        } catch (err) {
          /* 指针捕获已隐式释放: 忽略 */
        }
        document.body.classList.remove('cb-tree-resizing');
        // finish (up/cancel): 持久化偏好按硬边界截断, 绝不写入 50vw 截断值
        setTreeWidth(dragWidth, true);
      };
      treeResizer.addEventListener('pointermove', onMove);
      treeResizer.addEventListener('pointerup', onEnd);
      treeResizer.addEventListener('pointercancel', onEnd);
    });
    treeResizer.addEventListener('keydown', function (event) {
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
      setTreeWidth(next, true);
    });
    document.body.append(treeResizer);
    // 窄屏抽屉配套: 遮罩 + 唤起按钮 (宽屏 display:none, 无行为).
    // 遮罩点击收起; 按钮点击唤起 — 都走独立监听, 不干扰目录模式全局 handler.
    treeScrim = document.createElement('div');
    treeScrim.className = 'cb-tree-scrim';
    treeScrim.setAttribute('aria-hidden', 'true');
    treeScrim.addEventListener('click', closeTreeDrawer);
    document.body.append(treeScrim);
    treeOpenBtn = document.createElement('button');
    treeOpenBtn.type = 'button';
    treeOpenBtn.className = 'cb-tree-open';
    treeOpenBtn.setAttribute('aria-label', '打开目录树');
    treeOpenBtn.title = '打开目录树';
    treeOpenBtn.textContent = '☰ 目录';
    treeOpenBtn.addEventListener('click', openTreeDrawer);
    document.body.append(treeOpenBtn);
    installDrawerMql();
    return fetchTreeEntries('').then(function (entries) {
      if (gen !== treeGen) return;
      const nodes = entries.map(buildTreeNode);
      treeCache.set('', nodes);
      treeRootUl.replaceChildren(...nodes);
    });
  }

  /* ---------- 窄屏抽屉 (<64rem): off-canvas transform + 遮罩; 宽屏恒 no-op ---------- */

  function openTreeDrawer() {
    document.body.classList.add('cb-drawer-open');
    if (treeOpenBtn) {
      treeOpenBtn.setAttribute('aria-expanded', 'true');
    }
    // 焦点移入抽屉 (nav 非 tab 序, 仅程序化聚焦), 键盘导航立即可用
    if (treeNav) {
      try {
        treeNav.focus({ preventScroll: true });
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
    // 只有窄屏抽屉真实打开时才有焦点归还逻辑 (宽屏常驻侧栏不受影响)
    if (wasOpen && treeNav && treeNav.contains(document.activeElement)) {
      if (treeOpenBtn) {
        treeOpenBtn.focus({ preventScroll: true });
      } else {
        document.activeElement.blur();
      }
    }
  }

  // 窄屏 media query 监听: 跨回宽屏时清理打开态并重放持久化偏好 (窄屏阶段
  // 从未改写偏好, 恢复 CSS 变量与 ARIA); 收窄本身不得触碰偏好/生效宽.
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

  function uninstallDrawerMql() {
    if (drawerMql && drawerMqlHandler) {
      if (drawerMql.removeEventListener) {
        drawerMql.removeEventListener('change', drawerMqlHandler);
      } else if (drawerMql.removeListener) {
        drawerMql.removeListener(drawerMqlHandler);
      }
    }
    drawerMql = null;
    drawerMqlHandler = null;
  }

  /* ---------- SPA 内容区导航 (树模式) ---------- */

  // 内容区: 树模式右侧滚动容器. 进入树模式时把当前完整页内容 (目录页 main.cb-main
  // / 文档页 .cb-doc) 包装进 .cb-pane, 再 fetch 当前 URL 的 fragment 替换为统一
  // fragment 视图 (深链目录页显示迷你列表). 树内导航全部 SPA: fragment 注入 +
  // history.pushState/popstate, 无整页刷新; URL 恒为 actionUrl(path, 'full').
  let pane = null;
  let paneAbort = null; // 在途 fragment fetch 的 AbortController
  let paneCommittedPath = null; // 最近一次提交的 canonical encoded path (hash 导航防重取)
  let paneCommittedGen = null; // 最近一次提交的导航 generation (启动重同步用)
  let paneInitialGen = null; // 初始加载导航的 generation (启动链守卫用)
  // 单一活跃导航不变量: 单调 nav id (generation 守卫) — 仅当 id 最新且
  const paneNavGuard = createGenerationGuard();
  // pane 生命周期 registry: SPA 替换/退出树模式时统一拆除. ticket 08 的 TOC
  // observer 经 registerPaneDisposable 注册, 替换时自动 disconnect, 高亮不串台.
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

  // 统一 teardown: 先断开注册的 observer/媒体, 供替换与退出复用.
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
    return (
      document.querySelector('main.cb-main') ||
      document.querySelector('.cb-doc')
    );
  }

  function createPane() {
    const host = paneHostForPage();
    if (!host || pane) return null;
    const wrap = document.createElement('div');
    wrap.className = 'cb-pane';
    wrap.setAttribute('role', 'region');
    wrap.setAttribute('aria-label', '内容区');
    host.before(wrap);
    wrap.append(host);
    pane = wrap;
    return wrap;
  }

  function paneClearError() {
    if (!pane) return;
    const errEl = pane.querySelector(':scope > .cb-pane-error');
    if (errEl) errEl.remove();
  }

  // 错误只经 textContent (绝不 innerHTML): 保留现有 pane 内容, URL 不变.
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

  // 树内 SPA 导航: fetch fragment -> 校验 (ok + text/html + wrapper identity)
  // -> 提交 (teardown 旧 pane 状态, 安装新内容, pushState, 树状态同步).
  // 失败 fail-closed: 保留 pane 内容, 不 pushState (URL 不变), 文本错误.
  function paneNavigate(encodedPath, opts) {
    if (!isValidEncodedPath(encodedPath) || !pane) return null;
    const url = actionUrl(encodedPath, 'full');
    if (url === null) return null;
    const selectTree = !opts || opts.select !== false;
    const gen = paneNavGuard.begin(); // 新导航作废所有在途导航
    if (paneAbort) paneAbort.abort();
    const ac = new AbortController();
    paneAbort = ac;
    paneClearError();
    fetch(actionUrl(encodedPath, 'fragment'), {
      headers: { Accept: 'text/html' },
      signal: ac.signal,
    })
      .then(function (res) {
        if (!paneNavGuard.isCurrent(gen)) return null; // 过期: 丢弃
        return readFragmentResponse(res);
      })
      .then(function (html) {
        if (html === null || !paneNavGuard.isCurrent(gen)) return;
        commitPane(html, url, encodedPath, gen, selectTree, !!(opts && opts.pop));
      })
      .catch(function (err) {
        if (!paneNavGuard.isCurrent(gen)) return; // 过期错误: 丢弃
        if (err && err.name === 'AbortError') return; // 被新导航中止: 不报错
        if (opts && opts.pop) {
          // popstate: URL 已被浏览器改走, 旧内容与新 URL 不一致 — 统一 teardown
          // 后清空 pane (observer 断开 + 媒体暂停移除), 不绕过生命周期.
          teardownPane();
          pane.replaceChildren();
        }
        paneShowError(
          '无法加载：' + (err && err.message ? err.message : String(err))
        );
      });
    return gen;
  }

  // 提交守卫: id 最新 && wrapper identity 匹配 (fail-closed) && pushState 后
  // location.pathname 等于导航 canonical URL — 三者满足才动 DOM/history/树.
  function commitPane(html, url, encodedPath, gen, selectTree, isPop) {
    if (!paneNavGuard.isCurrent(gen)) return;
    const host = document.createElement('div');
    host.innerHTML = html; // 服务端 fragment (与预览面板同一信任契约)
    const wrapper = findFragmentWrapper(host);
    // 服务端 wrapper identity 是权威 canonical 形态; transport binding 要求
    // 请求路径与 wrapper 的 canonical key 精确相等 (均非 null), 否则 fail-closed
    // — 不接受任意合法 wrapper 路径. canonical key 仅用于比较, 绝不构造 action URL.
    const wrapperPath =
      wrapper && typeof wrapper.dataset.nativePathEncoded === 'string'
        ? wrapper.dataset.nativePathEncoded
        : null;
    const requestedKey = canonicalTransportKey(encodedPath);
    const wrapperKey = canonicalTransportKey(wrapperPath);
    const canonicalUrl =
      requestedKey !== null &&
      wrapperKey !== null &&
      requestedKey === wrapperKey &&
      isValidEncodedPath(wrapperPath)
        ? actionUrl(wrapperPath, 'full')
        : null;
    if (canonicalUrl === null) {
      // fail-closed: wrapper 缺失或 identity 非法 — 不推 URL
      if (isPop) {
        // popstate: URL 已被浏览器改走 — 统一 teardown 后清空, 内容不残留
        teardownPane();
        pane.replaceChildren();
      }
      paneShowError('无法加载：响应内容与目标不符');
      return;
    }
    // 初始加载 (gen === paneInitialGen) 恒 replaceState (规范化不新增历史);
    // 后续导航 pushState. hash 保留规则: 初始加载恒保留 (hash 属于所请求
    // 文档), 后续导航仅同文档 (同 pathname) 保留.
    const keepHash = gen === paneInitialGen || location.pathname === canonicalUrl.pathname;
    const target = canonicalUrl.href + (keepHash ? location.hash : '');
    if (location.href !== target) {
      if (gen === paneInitialGen) {
        history.replaceState(null, '', target);
      } else {
        history.pushState(null, '', target);
      }
    }
    teardownPane(); // 旧 observer 拆除 + 媒体卸载
    paneClearError();
    pane.replaceChildren(wrapper); // 移动解析出的 wrapper (替换全部旧内容)
    const titleEl = wrapper.querySelector('.cb-frag-title');
    if (titleEl && titleEl.textContent) {
      document.title = titleEl.textContent.trim();
    }
    pane.scrollTop = 0;
    // 文档布局: wrapper 即 .cb-doc (文档片段) 时直接装; 否则查内层 .cb-doc
    // (目录/媒体片段无 .cb-doc: installDocLayout(null) no-op, 不建 wrapper)
    if (wrapper.matches('.cb-doc')) {
      installDocLayout(wrapper);
    } else {
      installDocLayout(wrapper.querySelector('.cb-doc'));
    }
    installPaneTocSpy(wrapper); // 新内容就位后重建 pane TOC spy (无 TOC 则 no-op)
    paneCommittedPath = wrapperPath;
    paneCommittedGen = gen;
    syncTreeToPath(gen, selectTree);
    closeTreeDrawer(); // 导航提交成功: 窄屏抽屉收起 (宽屏 no-op)
  }

  function clearTreeCurrent() {
    if (!treeRootUl) return;
    $$('.cb-current', treeRootUl).forEach(function (el) {
      el.classList.remove('cb-current');
    });
  }

  // 同步树状态到当前 URL: 清除陈旧 cb-current, revealPath 展开祖先链, 完成后
  // 以当前链为准重标记; 用户导航 (selectTree) 时选中跟随 URL.
  function syncTreeToPath(gen, selectTree) {
    const encoded = currentPathFromLocation();
    if (encoded === null || !treeRootUl) return;
    const segments = encoded === '' ? [] : encoded.split('/');
    clearTreeCurrent();
    if (!paneNavGuard.isCurrent(gen)) return; // 陈旧导航: 树状态不跟进
    revealPath(segments, gen).then(function () {
      if (!paneNavGuard.isCurrent(gen)) return; // 过期: 树状态不跟进
      clearTreeCurrent(); // reveal 期间可能被更早链路标记: 重标记当前链
      const node = markTreePath(segments);
      if (selectTree) {
        if (node) {
          selectTreeNode(node);
        } else {
          clearTreeSelection(); // 选中跟随 URL: 无匹配节点则清空, 不残留旧选中
        }
      }
    });
  }
  function paneInitialLoad() {
    const encoded = currentPathFromLocation();
    if (encoded === null) return; // 非法路径: 保持完整页 (fail-closed)
    // 规范化由 commitPane 的初始 generation replaceState 完成 (wrapper
    // identity 为权威 canonical 形态, 含 hash 保留) — 此处不再自行 replaceState.
    paneInitialGen = paneNavigate(encoded, { select: false });
  }
  // back/forward (popstate): URL 已由浏览器同步, 直接按当前 location 导航;
  // 与最近提交路径相同 (hash 导航等) 不重取.
  function onPopState() {
    if (!treeMode || !pane) return;
    // popstate 一律先作废在途导航 (含 abort): 任何历史回退都取代当前 pending;
    // 目标已是最近提交内容时只作废不重取, 杜绝迟到 A 借 pushState 反提交.
    paneNavGuard.invalidate();
    if (paneAbort) {
      paneAbort.abort();
      paneAbort = null;
    }
    const encoded = currentPathFromLocation();
    if (encoded === null || encoded === paneCommittedPath) return;
    paneNavigate(encoded, { pop: true });
  }

  /* ---------- TOC scroll-spy (ticket 08) ---------- */

  // TOC 容器: markdown/org 自渲染为 #TOC; #table-of-contents 为 pandoc 兼容形态.
  function tocNavIn(root) {
    return root.querySelector('#TOC, #table-of-contents');
  }

  // 文档布局 (ticket 02): TOC 与正文是 .cb-doc 下的真实兄弟列. 幂等 — 已装过
  // 直接返回现有 layout. 原 #TOC / #table-of-contents 节点被移动 (绝不克隆),
  // 锚点 id / href / slug 契约不变. 无 TOC 文档返回 null, 不建 wrapper.
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
    // 窄屏 (无 rail) 不启动 spy (ticket 08 契约)
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
        // 短文档一屏放得下: 高亮首个标题 (阅读起点).
        setCurrent(links[targets[0].id] || null);
        return;
      }
      if (rootAtMaxScroll()) {
        // 滚动到底: 末节即当前节 (末标题无法越过阅读线的底边场景).
        setCurrent(links[last.id] || null);
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
        // 可滚动但未到底, 且全部标题都在可视区内 (标题集中在文档开头):
        // 高亮首个标题; 长文档未滚动到首个标题则无高亮.
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
    // 滚动到底监听: 末标题不越过观察带时 (无 IO 过渡) 也能高亮末节
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
  // 断点感知 spy 控制器: 跨 75rem 时按当前模式重装正确根的 spy (设计门 P1);
  // 窄屏恒无 spy (installTocSpy 的 matchMedia 门), 宽屏按模式各装各的.
  function reinstallCurrentSpy() {
    if (treeMode && pane) {
      if (paneSpyDisposer) {
        paneSpyDisposer();
        paneSpyDisposer = null;
      }
      const wrapper = pane.querySelector('.cb-doc');
      if (wrapper) installPaneTocSpy(wrapper);
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

  // 完整页 spy (目录模式文档页): root = 视口, 页面生命周期. disposer 存入模块
  // 变量, 进入树模式时断开 (原正文被 pane fragment 替换, 观察目标脱离 DOM).
  let fullPageSpyDisposer = null;
  function initFullPageTocSpy() {
    if (!document.body.classList.contains('cb-doc-page')) return;
    const docEl = document.querySelector('.cb-doc');
    installDocLayout(docEl); // 先装布局 (TOC 移入 toc 列), 再按新 DOM 装 spy
    const tocNav = docEl ? tocNavIn(docEl) : null;
    if (!tocNav) return; // 无 TOC 页面 (代码/无标题文档): 不启用 spy
    fullPageSpyDisposer = installTocSpy(docEl, tocNav, null);
  }

  function installPaneTocSpy(wrapper) {
    if (!pane) return;
    const tocNav = tocNavIn(wrapper);
    if (!tocNav) return; // 无 TOC fragment (代码/目录/媒体/占位): 不启用 spy
    const disposer = installTocSpy(wrapper, tocNav, pane);
    if (disposer) {
      paneSpyDisposer = disposer;
      registerPaneDisposable(function () {
        paneSpyDisposer = null;
        disposer();
      });
    }
  }
  function enterTreeMode() {
    // 完整页 spy 观察原正文: pane fragment 替换后目标脱离 DOM — 先断开
    if (fullPageSpyDisposer) {
      fullPageSpyDisposer();
      fullPageSpyDisposer = null;
    }
    closeTreeDrawer(); // 新壳抽屉默认收起 (宽屏 no-op)
    document.body.classList.add('cb-tree-mode');
    // 树宽: 恢复持久化偏好 (仅硬边界 [160,480] 截断, 解析失败/0 → 默认 238).
    // 宽屏立即按偏好应用 (此时 resizer 未建, setTreeWidth 只写变量 — 建壳时按
    // treeWidth 初始化 ARIA); 窄屏抽屉忽略宽度, 偏好原样保留, 跨回宽屏时由
    // drawer media handler 重放.
    treePreferredWidth = loadTreePreferredWidth();
    if (!window.matchMedia(TREE_DRAWER_MQL).matches) {
      setTreeWidth(treePreferredWidth, false);
    }
    // 目录模式预览面板在树模式不存在: 关闭并作废在途预览 (媒体/迟到响应不得残留)
    closePanel();
    // Q17 能力边界: 树模式不仅门禁事件路径, 更清空 JS 内存中的 token 并移除
    // 全部 native 控制 — 任何遗漏路径都无法再 POST. 退出树模式整页重载后
    // 重新从服务端读取 token (进程级同一 token, 重载只是重新获得能力);
    // 树模式的安全边界是清内存 + 删控制 + 事件门禁三重, 而非 token 轮换.
    nativeToken = null;
    const tokenMeta = document.querySelector('meta[name="cb-native-open-token"]');
    if (tokenMeta) tokenMeta.remove();
    $$('[data-cb-action="native"]').forEach(function (el) {
      el.remove();
    });
    // 内容区: 包装当前完整页内容进 .cb-pane, 再 fetch 当前 URL 的 fragment
    // 替换为统一 fragment 视图 (深链目录页显示迷你列表; 与外壳构建并行).
    createPane();
    paneInitialLoad();
    buildTreeShell().then(function () {
      // 启动期根 JSON 延迟时, 用户可能已通过 pane 迷你列表完成导航: 以最新
      // 已提交导航为准重同步树状态 (启动链的旧 generation 标记/选中不覆盖).
      if (
        paneCommittedGen !== null &&
        paneInitialGen !== null &&
        paneCommittedGen !== paneInitialGen
      ) {
        syncTreeToPath(paneCommittedGen, true);
        return;
      }
      const current = currentPathFromLocation();
      const reveal = current !== null
        ? revealPath(
            current === '' ? [] : current.split('/'),
            paneInitialGen
          )
        : Promise.resolve();
      return reveal.then(function () {
      // 先完成当前路径展开与持久化展开 (异步逐级加载), 再恢复选中 — 否则
      // 深层节点尚不存在, 选中恢复必然落空.
      const persisted = treeExpandedPaths.filter(isValidEncodedPath);
      const walks = persisted.map(function (path) {
          const segments = path.split('/');
          // 前缀正向遍历: 先在当前层找首个 segment, 展开后携带前缀下钻,
          // 绝不反向切片 (saved 'a/b' 必须先匹配 /a 再匹配 /a/b).
          const walk = function (ul, segs, prefix) {
            if (!ul || segs.length === 0) return Promise.resolve();
            const stepHref = '/' + prefix.concat([segs[0]]).join('/');
            const node = $$(':scope > li', ul).find(function (li) {
              return (
                isExpandableDir(li) &&
                nodeHref(li) !== null &&
                nodeHref(li).pathname === stepHref
              );
            });
            if (!node) return Promise.resolve();
            const nodePathV = encodedPathOf(node);
            if (nodePathV === null) return Promise.resolve();
            return loadTreeChildren(node, nodePathV).then(function () {
              return walk(
                node.querySelector(':scope > ul.cb-tree-children'),
                segs.slice(1),
                prefix.concat([segs[0]])
              );
            });
          };
          return walk(treeRootUl, segments, []);
        });
        return Promise.all(walks);
      });
    }).then(function () {
      // 启动链的选中恢复只属于初始导航 generation: 用户已导航走则不覆盖
      if (paneInitialGen === null || paneNavGuard.isCurrent(paneInitialGen)) {
        restoreTreeSelection();
      }
    });
  }

  function exitTreeMode() {
    const hadPane = pane !== null;
    treeGen += 1; // 使全部在途树响应失效 (旧壳 continuation 不得触碰新壳)
    document.body.classList.remove('cb-tree-mode');
    if (hadPane) {
      paneNavGuard.invalidate(); // 在途 fragment 响应/错误不得安装状态
      if (paneAbort) paneAbort.abort();
      teardownPane(); // observer 拆除 + 媒体卸载
      pane.remove();
      pane = null;
      paneCommittedPath = null;
      paneCommittedGen = null;
      paneInitialGen = null;
    }
    closeTreeDrawer();
    uninstallDrawerMql();
    // 树宽调整器与 CSS 变量随壳销毁: 残留变量会污染再进入时的默认列 (变量
    // 只在树模式被 JS 重新写入, 但避免 dir 宿主残留样式差异).
    if (treeResizer) {
      treeResizer.remove();
      treeResizer = null;
    }
    document.body.style.removeProperty('--cb-tree-width');
    document.body.classList.remove('cb-tree-resizing');
    if (treeScrim) treeScrim.remove();
    treeScrim = null;
    if (treeOpenBtn) treeOpenBtn.remove();
    treeOpenBtn = null;
    if (treeNav) treeNav.remove();
    treeNav = null;
    treeRootUl = null;
    treeCache.clear();
    clearTreeSelection();
    // SPA 期间 pane 内容已被 fragment 替换: 离开树模式重载当前 URL 恢复完整页.
    // 仅在真实离开树模式时 (init 时 treeMode 为 false 则 pane 不存在, 不重载).
    if (hadPane) location.reload();
  }

  function applyTreeMode() {
    const btn = $('.cb-mode-toggle');
    if (btn) {
      btn.textContent = treeMode ? '目录' : '树';
      btn.setAttribute('aria-pressed', treeMode ? 'true' : 'false');
      btn.title = treeMode ? '切换到目录模式' : '切换到树模式';
    }
    if (treeMode) {
      enterTreeMode();
    } else {
      exitTreeMode();
    }
  }

  function injectModeToggle() {
    if (!isTreeCapablePage() || $('.cb-mode-toggle')) return;
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'cb-mode-toggle';
    btn.addEventListener('click', function () {
      treeMode = !treeMode;
      try {
        if (treeMode) {
          localStorage.setItem(MODE_KEY, 'tree');
        } else {
          localStorage.removeItem(MODE_KEY);
        }
      } catch (err) {
        /* 持久化失败忽略 */
      }
      applyTreeMode();
    });
    document.body.append(btn);
  }

  function onTreeKeydown(event) {
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    // Escape 先于 editable 守卫 (镜像目录模式): 聚焦树搜索框时先失焦, 否则清除选中
    if (event.key === 'Escape') {
      const input = $('#cb-tree-search-q');
      if (document.activeElement === input) {
        input.blur();
      } else {
        clearTreeSelection();
      }
      closeTreeDrawer(); // 窄屏抽屉: Esc 收起 (宽屏 no-op)
      event.preventDefault();
      return;
    }
    if (isEditableTarget(event.target)) return;
    switch (event.key) {
      case 'ArrowDown':
        moveTreeSelection(1);
        event.preventDefault();
        break;
      case 'ArrowUp':
        moveTreeSelection(-1);
        event.preventDefault();
        break;
      case 'ArrowRight':
        // → 只展开 (契约: → 展开 / ← 收起); 已展开则 no-op
        if (treeSelectedEl && isExpandableDir(treeSelectedEl)) {
          const ul = treeSelectedEl.querySelector(':scope > ul.cb-tree-children');
          if (!ul || ul.hidden) {
            const path = encodedPathOf(treeSelectedEl);
            if (path !== null) loadTreeChildren(treeSelectedEl, path);
          }
          event.preventDefault();
        }
        break;
      case 'ArrowLeft': {
        if (treeSelectedEl) {
          const ul = treeSelectedEl.querySelector(':scope > ul.cb-tree-children');
          if (isDirNode(treeSelectedEl) && ul && !ul.hidden) {
            setTreeNodeExpanded(treeSelectedEl, false);
          } else {
            const row = treeSelectedEl.closest('ul');
            const parentLi = row ? row.closest('li') : null;
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
        const current = currentPathFromLocation();
        if (current !== null && current !== '') {
          const segments = current.split('/');
          const parent = segments.slice(0, -1).join('/');
          if (pane) {
            paneNavigate(parent);
          } else {
            location.href = '/' + parent;
          }
        }
        event.preventDefault();
        break;
      }
      case '/': {
        const input = $('#cb-tree-search-q');
        if (input) input.focus();
        event.preventDefault();
        break;
      }
      default:
        break;
    }
  }

  function init() {
    const tokenMeta = document.querySelector(
      'meta[name="cb-native-open-token"]'
    );
    nativeToken = tokenMeta ? tokenMeta.getAttribute('content') : null;
    injectModeToggle();
    // 完整页 spy 先于 applyTreeMode: 树模式启动时 enterTreeMode 立即断开它
    // (原正文将被 pane fragment 替换), 避免残留观察已脱离 DOM 的标题.
    initFullPageTocSpy();
    installTocSpyMql(); // 断点感知 spy 控制器 (跨 75rem 重装, 页面生命周期)
    if (isTreeCapablePage()) {
      try {
        treeMode = localStorage.getItem(MODE_KEY) === 'tree';
      } catch (err) {
        treeMode = false;
      }
      applyTreeMode();
    }
    applyView();
    setActionsEnabled(false); // 初始无 panel target
    document.addEventListener('click', onDocumentClick);
    document.addEventListener('dblclick', onDocumentDblclick);
    document.addEventListener('keydown', onKeydown);
    window.addEventListener('popstate', onPopState);
    $$('[data-cb-view]').forEach(function (btn) {
      btn.addEventListener('click', function () {
        view = btn.getAttribute('data-cb-view');
        applyView();
      });
    });
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
    };
  }
})();
