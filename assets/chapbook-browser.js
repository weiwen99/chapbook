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

  // 并发预览的 generation 守卫 (DOM 无关 seam): begin() 使旧 generation 过期;
  // isCurrent(gen) 判定在途响应/错误是否仍属于最新请求; invalidate() 使一切
  // 在途响应/错误失效 (面板关闭时调用, 迟到的响应/错误不得安装任何状态)。
  function createPreviewGuard() {
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
  const previewGuard = createPreviewGuard(); // 并发预览 generation 守卫

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
        if (!res.ok) throw new Error('HTTP ' + res.status);
        const ct = res.headers.get('Content-Type') || '';
        if (!/text\/html/i.test(ct)) throw new Error('响应不是 HTML');
        return res.text();
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
    // 面板内目录迷你列表: 拦截可操作 anchor 做面板内下钻
    const miniLink = event.target.closest(
      '.cb-dir-mini a[data-native-path-encoded]'
    );
    if (miniLink) {
      event.preventDefault();
      openPreview(encodedPathOf(miniLink));
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
    // P2#2: Escape 先于通用 editable-target 守卫处理 — 聚焦在搜索框时先失焦,
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

  function init() {
    const tokenMeta = document.querySelector(
      'meta[name="cb-native-open-token"]'
    );
    nativeToken = tokenMeta ? tokenMeta.getAttribute('content') : null;
    applyView();
    setActionsEnabled(false); // 初始无 panel target
    document.addEventListener('click', onDocumentClick);
    document.addEventListener('dblclick', onDocumentDblclick);
    document.addEventListener('keydown', onKeydown);
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
      createPreviewGuard: createPreviewGuard,
    };
  }
})();
