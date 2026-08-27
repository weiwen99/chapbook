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

  // 树宽拖拽手势所有权: 仅主指针左键可在无在途手势时开始 (右键/中键/副指针/
  // 多指一律拒绝); move/end 只认持有手势的 pointerId, 其他指针不得移动或
  // 结束在途拖拽. isPrimary 省略视同主指针 (合成事件/键盘可达).
  function canStartTreeResize(event, activePointerId) {
    return activePointerId === null && event.button === 0 && event.isPrimary !== false;
  }
  function ownsTreeResize(event, activePointerId) {
    return activePointerId !== null && event.pointerId === activePointerId;
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
    // 树节点: 单击打开 — 目录展开/收起, 文件 SPA 导航; 双击连击 (detail > 1)
    // 不产生第二次动作 (第二次 click 直接返回; dblclick 分支同样阻断)
    const treeRow = event.target.closest('.cb-tree-row');
    if (treeRow) {
      if (event.detail > 1) return; // 双击连击: 单击已处理, 不产生第二次动作
      const li = treeRow.closest('li.cb-tree-node');
      if (!li) return;
      focusTreeRootUl(); // 先落焦点: active-descendant 公告 + 键盘可立即接管
      selectTreeNode(li);
      if (isDirNode(li)) toggleTreeNode(li); else navigateTreeNode(li);
      closeTreeDrawer(); return; // 窄屏抽屉: 选择条目后收起 (宽屏 no-op)
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
    // 树节点双击: 单击已打开 (目录展开/文件导航), 双击必须无第二次动作 —
    // 阻断默认并返回, 不再整页/SPA 导航
    if (event.target.closest('.cb-tree-row')) {
      event.preventDefault();
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
  // 在途目录加载去重: path -> 在途 Promise. 同一目录只允许一个在途请求
  // (重复激活返回同一 Promise, 绝不二次 fetch); 条目只由 owner Promise 的
  // finally 清理, 退出树模式整体清空.
  const treePendingLoads = new Map();
  // 目录展开意图: path -> { expanded, version, explicit } 记录. 按发布时序
  // (全局单调 treeExpansionIntentVersion) 版本化 — 最新记录即终态权威: 更新
  // 的导航 (explicit=false) 覆盖更早的显式折叠, 更晚的显式点击覆盖导航.
  // 安装方 (installTreeChildren) 在落地终态后按记录身份消费; 失败响应 (无
  // 缓存) 兜底只删本请求适用记录; 叶子化/退出壳整体清除.
  const treeExpansionIntents = new Map();
  // 全局单调版本: 每次发布意图递增 — 跨路径共享时序; 记录比较只用身份/版本
  // (跨壳不清零: 记录已随壳清空, 身份比较天然隔离).
  let treeExpansionIntentVersion = 0;
  let treeSelectedEl = null;
  // 稳定节点 id 计数器: 仅保证当前壳内唯一/稳定 (ARIA 活动后代公告锚点);
  // 每次建壳复位 (退出树模式后旧节点随壳销毁, id 可安全复用).
  let treeItemSeq = 0;
  // 树壳生命周期 generation: 退出树模式递增使全部在途响应失效 (镜像 previewGuard)
  let treeGen = 0;
  // 本壳树请求 (root/子项) 的 AbortController: 进入时创建 (先于 root/子项
  // 请求), 退出时 abort 并置空 — 在途请求以 AbortError 收尾, 失败处理器静默
  // 吞掉 (无可见陈旧 alert、无未处理拒绝).
  let treeAbort = null;
  // 窄屏抽屉 (<64rem, off-canvas transform + 遮罩): 随树壳创建/销毁.
  // 开合态挂 body.cb-drawer-open (宽屏无对应 CSS, 恒 no-op).
  let treeScrim = null; // 遮罩: 点击收起抽屉
  let treeOpenBtn = null; // 唤起按钮 (仅窄屏可见)
  let drawerMql = null; // 窄屏 media query (跨回宽屏时清理打开态)
  let drawerMqlHandler = null;
  // 宽屏视口 resize 重钳制: 视口收窄 (仍 ≥64rem) 时 50vw 上限下降, 生效宽
  // 需跟随重钳制; 命名处理器便于 enter/exit 对称安装卸载 (绝不写 localStorage).
  let treeViewportResizeHandler = null;
  // 树宽 (宽屏列 1): resizer 常驻调整; 窄屏抽屉内 resizer 隐藏, 宽度值不生效.
  // treePreferredWidth 是持久化偏好: 仅硬边界 [160,480] 截断, 绝不被窄屏 50vw
  // 上限改写; treeWidth 是当前生效宽 (宽屏 = 偏好按视口约束, 窄屏抽屉忽略).
  let treePreferredWidth = TREE_DEFAULT_WIDTH;
  let treeWidth = TREE_DEFAULT_WIDTH;
  let treeResizer = null; // 列 2 调整器元素 (随树壳创建/销毁)
  // 在途拖拽手势的 pointerId (单 owner): 无手势时为 null. move/end/收尾只认
  // 该 id; 捕获失败或退出树模式时干净复位.
  let treeResizePointerId = null;
  // 手动折叠 (宽屏左树收起): 偏好与生效态分离. treeCollapsePreferred 是持久化
  // 偏好 (仅 '1' 折叠); treeCollapsed 是当前生效态 — 窄屏抽屉恒展开, 宽屏 =
  // 偏好 (跨断点由 mql handler 重放). 折叠期间树/调整器隐藏, 左缘恢复按钮
  // (treeRestoreBtn) 悬浮常驻.
  const TREE_COLLAPSED_KEY = 'cb-tree-collapsed';
  let treeCollapsePreferred = false;
  let treeCollapsed = false;
  let treeRestoreBtn = null; // 左缘恢复按钮 (仅折叠生效时可见)
  let treeCollapseMql = null; // 断点感知: 窄屏强制展开, 宽屏重放偏好
  let treeCollapseMqlHandler = null;
  let treeCollapseBtn = null; // 折叠按钮 (树壳顶栏子控件; 展开态焦点落点, 随壳创建/销毁)
  // 目录模式左缘工具栏行与模式切换按钮: 跨模式复用 (按钮在树顶栏与目录工具
  // 栏行之间移动, 随目录工具栏行创建/销毁; 树模式隐藏该行, 退出恢复).
  let modeToolbar = null;
  let modeToggleBtn = null;
  // 树壳焦点追踪: 跨断点时浏览器在 mql 处理器运行前就按新视口 CSS 把目标
  // 控件 display:none 并静默丢弃焦点 (无 focusout 事件), 处理器启动时
  // activeElement 已回落 body. focusin 捕获阶段记录最近一次落在树壳控件上的
  // 焦点种类, 落到非树壳控件 (键盘移出/指针点击内容区) 即清空; applyTreeCollapsed
  // 仅在 activeElement 已丢失焦点时用它补判跨断点落点, 不会因过期记录抢焦点.
  let treeFocusKind = null;
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

  function treeSearchForm(inputId) {
    const form = document.createElement('form');
    form.className = 'cb-tree-search';
    form.action = '/__/search';
    form.method = 'get';
    form.setAttribute('role', 'search');
    const input = document.createElement('input');
    input.type = 'search';
    input.name = 'q';
    input.id = inputId || 'cb-tree-search-q';
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
    li.id = 'cb-tree-item-' + (++treeItemSeq); // 稳定 id: ARIA 活动后代公告锚点
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
    if (isExpandableDir(li)) li.setAttribute('aria-expanded', expanded ? 'true' : 'false');
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
  // signal: 本壳 AbortController 的取消信号 — 壳退出即中止在途树请求.
  function fetchTreeEntries(path, signal) {
    const url = path === '' ? '/__/tree' : '/__/tree/' + path;
    const opts = { headers: { Accept: 'application/json' } };
    if (signal) opts.signal = signal;
    return fetch(url, opts)
      .then(function (res) {
        if (!res.ok) throw new Error('HTTP ' + res.status);
        return res.json();
      })
      .then(function (data) {
        return data && Array.isArray(data.entries) ? data.entries : [];
      });
  }

  // 展开意图纯函数 (DOM 无关 seam): 显式用户激活的下一条意图. prior 为最新
  // 已发布记录或 null. 最新记录为显式用户意图 (explicit=true) 时翻转其
  // expanded (第二次激活表达收起); 其余 (无记录 / nav 默认 explicit=false)
  // 按可见 DOM 推导 (!expanded — 首击可见折叠目录即表达展开, nav 的隐藏意图
  // 绝不参与). 返回 { expanded, explicit: true } — 调用方经
  // publishTreeExpansionIntent 发布 (版本化: 更晚的显式动作自然覆盖更早导航).
  function treeNextExplicitIntent(prior, expanded) {
    if (prior && prior.explicit) return { expanded: !prior.expanded, explicit: true };
    return { expanded: expanded ? false : true, explicit: true };
  }

  // 发布展开意图 (唯一写入点): 递增全局单调版本并存储记录, 返回记录. 导航/
  // 默认发布 (explicit=false) 与显式用户发布 (explicit=true) 共用 — 时序即
  // 优先级: 更新的记录永远覆盖更旧的 (latest wins).
  function publishTreeExpansionIntent(path, expanded, explicit) {
    const record = {
      expanded: expanded,
      version: ++treeExpansionIntentVersion,
      explicit: explicit,
    };
    treeExpansionIntents.set(path, record);
    return record;
  }

  // 树加载提交决策纯函数 (DOM 无关 seam): 提交 DOM/展开/持久化前必须通过 —
  // 壳当前 (shellCurrent) 恒必需; 带导航所有权 (navRequired, reveal/sync 链)
  // 时还须导航当前 (navCurrent); 显式用户调用 (无导航所有权) 只要求壳当前.
  // 缓存结果共享不受此约束 (陈旧导航可复用结果, 但不得提交 DOM).
  function treeLoadMayCommit(shellCurrent, navRequired, navCurrent) {
    if (!shellCurrent) return false;
    return !navRequired || navCurrent;
  }

  // 空目录响应: li 转为终端叶子 — 无子项无展开语义, 未来 click/ArrowRight 经
  // isExpandableDir 直接短路, 不再发起请求/翻转意图. 移除 group/展开属性与
  // 持久化展开, 意图随叶子删除 (空目录不保留展开状态).
  function convertTreeLeaf(li, path) {
    const ul = li.querySelector(':scope > ul.cb-tree-children');
    // 叶子化移除的子树内若含选中节点: 先清选中与 aria-activedescendant
    if (ul && treeSelectedEl && ul.contains(treeSelectedEl)) {
      clearTreeSelection();
    }
    li.dataset.treeExpandable = '0';
    li.removeAttribute('aria-expanded');
    const twist = li.querySelector(':scope > .cb-tree-row .cb-tree-twist');
    if (twist) twist.textContent = '';
    if (ul) ul.remove(); // 子 role=group 一并移除: 叶子不得携带 group 语义
    const before = treeExpandedPaths.length;
    treeExpandedPaths = treeExpandedPaths.filter(function (p) {
      return p !== path;
    });
    if (treeExpandedPaths.length !== before) {
      saveJsonArray(TREE_EXPANDED_KEY, treeExpandedPaths);
    }
    treeExpansionIntents.delete(path); // 叶子无展开语义: 意图记录一并清除
  }

  // 唯一子项替换点: 把缓存子项安装进 li (缓存命中与异步响应共用). 空缓存 →
  // 叶子化; 替换前先清除被替换子树内的选中 (aria-activedescendant 同步移除);
  // 终态跟随最新意图记录 (在途期间可翻转表达收起), 无记录默认展开, 绝不盲目
  // 折叠; 落地终态后按记录身份消费 (DOM 即权威) — 续接方晚于 owner finally
  // 注册, 意图必须由安装方清理, 否则 finally 抢先清除会让续接方读到已清除的
  // 意图; 身份比对绝不误删更新导航/显式点击发布的更新记录.
  function installTreeChildren(li, path) {
    const cached = treeCache.get(path);
    if (!cached) return;
    if (cached.length === 0) {
      convertTreeLeaf(li, path);
      return;
    }
    const ul = childrenUlFor(li);
    if (treeSelectedEl && ul.contains(treeSelectedEl)) {
      clearTreeSelection();
    }
    ul.replaceChildren(...cached);
    const record = treeExpansionIntents.get(path);
    setTreeNodeExpanded(li, record ? record.expanded : true);
    if (treeExpansionIntents.get(path) === record) {
      treeExpansionIntents.delete(path); // 意图已消费: DOM 即权威, 不留残留
    }
  }

  // 共享在途结果的续接方提交: 响应可能因发起方所有权陈旧只写缓存未动 DOM —
  // 续接方所有权当前时补齐安装; 已安装 (子项非空) 则跳过, 保证同一 DOM/壳内
  // 只有一个调用方执行 replaceChildren.
  function ensureTreeChildrenInstalled(li, path) {
    if (!treeCache.has(path)) return;
    const ul = li.querySelector(':scope > ul.cb-tree-children');
    if (ul && ul.childElementCount > 0) return;
    installTreeChildren(li, path);
  }

  // 懒加载目录子项; 失败静默 (网络/服务端错误不留半展开态). 同一目录路径只
  // 允许一个在途请求: 重复激活复用同一 Promise, 绝不二次 fetch; 每个调用方
  // 都在共享 Promise 上挂自己的续接 — 发起方所有权陈旧时 (如陈旧导航) 响应
  // 只写缓存不落 DOM, 本调用方所有权当前则补齐安装, 终态跟随最新意图记录
  // (treeExpansionIntents, 无记录默认展开), 绝不盲目折叠; 响应落地后安装方
  // (installTreeChildren) 在落地终态后按记录身份消费意图 (DOM 即权威), finally
  // 在 owner 块内清除在途条目, 失败响应 (无缓存) 才由 finally 兜底删除本请求
  // 适用记录 (旧壳 finally 不得删新壳条目; 续接方晚于 finally 注册, 意图不得
  // 被 finally 抢先清除; 更新记录绝不被删除).
  // owner 传调用方所有权: { navGen } (reveal/sync 导航链) 或 { enterGen }
  // (进入链持久化展开) — 响应落地时导航/进入链已过期则只共享缓存结果, 不
  // 提交 DOM/展开/持久化; owner 存在时每次调用都发布 nav/默认意图
  // (expanded=true, explicit=false) — 更新的导航覆盖更早的显式折叠; 无 owner
  // (显式用户调用) 可采纳共享在途结果, 意图已由调用方发布.
  function loadTreeChildren(li, path, owner) {
    const gen = treeGen;
    const navGen = owner && typeof owner.navGen === 'number' ? owner.navGen : undefined;
    const enterGen = owner && typeof owner.enterGen === 'number' ? owner.enterGen : undefined;
    const navRequired = navGen !== undefined;
    const navCurrent = navRequired && paneNavGuard.isCurrent(navGen);
    // nav/进入链 (owner 存在) 每次请求/复用祖先展开都发布 nav/默认意图 — 时序
    // 即优先级: 更新的导航 (explicit=false) 覆盖更早的显式折叠; 显式用户调用
    // (无 owner) 已由 toggleTreeNode/ArrowRight 发布显式意图, 此处绝不覆盖.
    if (owner) publishTreeExpansionIntent(path, true, false);
    if (treeCache.has(path)) {
      // 缓存结果可共享: 陈旧导航/进入链只取结果, 不提交 DOM/展开/持久化
      if (!treeLoadMayCommit(enterGen === undefined || enterGen === treeGen, navRequired, navCurrent)) {
        return Promise.resolve();
      }
      installTreeChildren(li, path);
      return Promise.resolve();
    }
    const inFlight = treePendingLoads.get(path);
    if (inFlight) {
      // 每个调用方都挂自己的续接: 发起方所有权陈旧时 (如陈旧导航) 响应只写缓存
      // 不落 DOM — 本调用方所有权当前则补齐安装, 终态跟随最新意图记录 (缓存
      // 结果共享); 绝不 bare-return 发起方的 Promise. 所有权在响应落地时重查:
      // 调用时的快照 (navCurrent/壳代) 会过期 — 跨导航/跨壳后不得提交.
      return inFlight.then(function (entries) {
        if (!treeLoadMayCommit(gen === treeGen, navRequired, paneNavGuard.isCurrent(navGen))) {
          return entries;
        }
        ensureTreeChildrenInstalled(li, path); // 空缓存 → 叶子化; 已安装 → 跳过
        return entries;
      });
    }
    // 请求起始记录 (版本 token): 失败兜底只删本请求适用记录, 绝不删除更新
    // 导航/显式点击发布的更新记录.
    const intentRecord = treeExpansionIntents.get(path);
    const pending = fetchTreeEntries(path, treeAbort ? treeAbort.signal : undefined).then(
      function (entries) {
        // 旧壳/旧进入链在途响应: 不得写缓存、不得触碰新壳 DOM
        if (gen !== treeGen || (enterGen !== undefined && enterGen !== treeGen)) return;
        const nodes = entries.map(buildTreeNode);
        treeCache.set(path, nodes); // 结果可共享: 缓存先于所有权提交
        // 导航所有权: 陈旧导航只共享缓存结果, 不提交 DOM/展开/持久化
        if (navRequired && !paneNavGuard.isCurrent(navGen)) return;
        installTreeChildren(li, path);
      },
      function (err) {
        /* 加载失败: 不标记展开, 下次点击重试; 壳退出中止 (AbortError) 静默 */
        if (err && err.name === 'AbortError') return;
      }
    );
    treePendingLoads.set(path, pending);
    pending.finally(function () {
      // 清理只由 owner Promise 执行: 先比对在途条目身份 (恒为本 Promise) —
      // 旧壳/旧链的 finally 不得删除新壳注册的条目与意图. 成功响应已由安装方
      // (installTreeChildren) 在落地终态后按记录身份消费; 此处兜底只清无消费
      // 方的意图: 失败响应 (无缓存) 且当前记录仍是本请求起始记录 (身份相同)
      // 才删除 — 绝不删除更新导航/显式点击发布的更新记录 (latest wins); 成功
      // 落地但无安装方 (陈旧导航只写缓存, 有缓存) 不删 — 后续发布恒新版本,
      // 陈旧 nav 记录不影响任何决策.
      if (treePendingLoads.get(path) === pending) {
        treePendingLoads.delete(path);
        if (!treeCache.has(path) && treeExpansionIntents.get(path) === intentRecord) {
          treeExpansionIntents.delete(path);
        }
      }
    });
    return pending;
  }

  function toggleTreeNode(li) {
    if (!isExpandableDir(li)) return;
    const ul = li.querySelector(':scope > ul.cb-tree-children');
    const path = encodedPathOf(li);
    if (path === null) return;
    const expanded = !!ul && !ul.hidden;
    const pending = treePendingLoads.has(path);
    if (pending) {
      // 在途: 经意图纯函数按最新记录推导 — prior 显式则翻转 (第二次激活表达
      // 收起), prior 无/nav 默认按可见 DOM 推导 (!expanded, nav 的隐藏意图
      // 绝不参与 — 首击可见折叠目录即表达展开); 发布显式意图 (版本化) 后恒经
      // loadTreeChildren 挂 ownerless 续接 (在途去重: 绝不发起第二个请求) —
      // 响应落地时由续接方安装, 终态跟随最新记录, 安装方消费意图后 DOM 成为
      // 权威. 绝不裸 return: 无续接时陈旧 nav owner 的响应不会安装, 用户点击
      // 将落空 (需要第三次点击).
      const next = treeNextExplicitIntent(treeExpansionIntents.get(path), expanded);
      publishTreeExpansionIntent(path, next.expanded, true);
      return loadTreeChildren(li, path);
    }
    // 无在途: 意图无消费方 — 收起直接改 DOM (绝不写入残留, 陈旧意图会挡住
    // 后续 reveal 的缓存命中展开); 展开经纯函数 + 发布显式意图后加载.
    if (expanded) {
      setTreeNodeExpanded(li, false);
    } else {
      // 用户激活即显式意图 (nav/reveal 默认不算): 经最新记录推导并发布 —
      // 在途期间的后续激活按显式记录翻转 (第二次激活表达收起), 而不是把首击
      // 当 nav 默认意图翻转.
      const next = treeNextExplicitIntent(treeExpansionIntents.get(path), expanded);
      publishTreeExpansionIntent(path, next.expanded, true);
      loadTreeChildren(li, path);
    }
  }

  function selectTreeNode(li) {
    if (treeSelectedEl) {
      treeSelectedEl.classList.remove('cb-selected');
      treeSelectedEl.setAttribute('aria-selected', 'false');
    }
    treeSelectedEl = li;
    li.classList.add('cb-selected');
    li.setAttribute('aria-selected', 'true');
    if (treeRootUl) treeRootUl.setAttribute('aria-activedescendant', li.id);
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
    if (treeSelectedEl) {
      treeSelectedEl.classList.remove('cb-selected');
      treeSelectedEl.setAttribute('aria-selected', 'false');
    }
    treeSelectedEl = null;
    if (treeRootUl) treeRootUl.removeAttribute('aria-activedescendant');
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
    const shellGen = treeGen; // 链级壳守卫: 退出/重进后旧链路立即终止
    let parentUl = treeRootUl;
    let lastNode = null;
    let promise = Promise.resolve();
    segments.forEach(function (seg, i) {
      const href = '/' + segments.slice(0, i + 1).join('/');
      promise = promise.then(function () {
        // 旧壳链路: 立即终止 (退出/重进后不得触碰新壳 DOM)
        if (shellGen !== treeGen) {
          parentUl = null;
          return;
        }
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
          // 导航所有权随加载传递: 陈旧导航响应只共享缓存结果
          return loadTreeChildren(node, path, { navGen: gen }).then(function () {
            if (shellGen !== treeGen) {
              parentUl = null;
              return;
            }
            // 续接方所有权当前才补齐安装 (发起方陈旧时响应只写缓存)
            if (gen !== undefined && !paneNavGuard.isCurrent(gen)) return;
            ensureTreeChildrenInstalled(node, path);
            parentUl = node.querySelector(':scope > ul.cb-tree-children');
          });
        }
        parentUl = null;
      });
    });
    return promise.then(function () {
      // 旧壳链路: 显式陈旧哨兵 (调用方不得按成功处理)
      if (shellGen !== treeGen) return null;
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
  // 恢复持久化偏好: 仅有限正数合法 (0/NaN/Infinity/负数 → 默认 238), 再按硬边界截断.
  function loadTreePreferredWidth() {
    let saved = 0;
    try {
      saved = Number(localStorage.getItem(TREE_WIDTH_KEY));
    } catch (err) {
      /* 持久化读取失败: 用默认值 */
    }
    return Number.isFinite(saved) && saved > 0 ? clampTreePreferred(saved) : TREE_DEFAULT_WIDTH;
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
    treeNav.id = 'cb-tree-nav'; // 稳定 id: 抽屉唤起按钮 aria-controls 锚点
    treeNav.setAttribute('aria-label', '目录树');
    // 顶栏: 搜索表单 + 折叠按钮, 作为树壳第一个子节点 (先于根列表). 折叠按钮
    // 随树壳 inert/hidden/抽屉生命周期一起走, 不再是调整器子控件; 调整器上的
    // pointerdown/keydown 守卫 (closest('.cb-tree-collapse')) 保留为防御纵深.
    const treeToolbar = document.createElement('div');
    treeToolbar.className = 'cb-tree-toolbar';
    treeToolbar.append(treeSearchForm());
    treeCollapseBtn = document.createElement('button');
    treeCollapseBtn.type = 'button';
    treeCollapseBtn.className = 'cb-tree-collapse';
    treeCollapseBtn.setAttribute('aria-label', '收起目录树');
    treeCollapseBtn.title = '收起目录树';
    treeCollapseBtn.textContent = '‹';
    treeCollapseBtn.addEventListener('click', function (event) {
      event.stopPropagation(); // 不落入 document 全局 click (native action 等)
      setTreeCollapsePreference(true, true);
    });
    treeToolbar.append(treeCollapseBtn);
    // 模式切换按钮: 顶栏末位 (append 移动节点, 自目录工具栏行迁入; 随树壳
    // inert/hidden/抽屉生命周期一起走). 退出树模式时迁回目录工具栏行.
    treeToolbar.append(modeToggleBtn);
    treeNav.append(treeToolbar);
    treeItemSeq = 0; // 新壳: id 计数器复位 (id 仅需壳内稳定唯一, 见声明)
    treeRootUl = document.createElement('ul');
    treeRootUl.className = 'cb-tree-root';
    treeRootUl.id = 'cb-tree-root'; // 稳定 id: 树根 (单 tab 停靠点) 身份锚点
    treeRootUl.setAttribute('role', 'tree');
    treeRootUl.setAttribute('aria-label', '目录树');
    treeRootUl.tabIndex = 0; // 单 tab 停靠点; 活动后代经 aria-activedescendant 公告
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
      // 折叠按钮守卫 (防御纵深, 按钮已移入树壳顶栏): 子控件不触发拖拽, 先于一切返回.
      if (event.target.closest('.cb-tree-collapse')) return;
      // 仅主指针左键可开始; 已有在途手势时拒绝 (副键/多指不抢手势).
      if (!canStartTreeResize(event, treeResizePointerId)) return;
      // 先捕获后启动: 捕获失败 (异常/禁用) 干净返回, 不留 class/监听/持久化.
      try {
        treeResizer.setPointerCapture(event.pointerId);
      } catch (err) {
        return;
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
      // 先复位 owner 再释放捕获 — releasePointerCapture 触发的
      // lostpointercapture 重入此刻已无 owner, 直接返回, 不重复清理/持久化.
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
        // finish (up/cancel): 持久化偏好按硬边界截断, 绝不写入 50vw 截断值
        setTreeWidth(dragWidth, true);
      };
      treeResizer.addEventListener('pointermove', onMove);
      treeResizer.addEventListener('pointerup', onEnd);
      treeResizer.addEventListener('pointercancel', onEnd);
      treeResizer.addEventListener('lostpointercapture', onEnd);
    });
    treeResizer.addEventListener('keydown', function (event) {
      // 折叠按钮守卫 (防御纵深) 的按键 (Enter/Space 触发 click) 不得被当成宽度步进.
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
    return fetchTreeEntries('', treeAbort ? treeAbort.signal : undefined).then(
      function (entries) {
        if (gen !== treeGen) return;
        // 重试/成功即清理: 既有错误元素 (同壳重取/迟到清理) 一律移除
        const prevErr = treeNav.querySelector('.cb-tree-error');
        if (prevErr) prevErr.remove();
        const nodes = entries.map(buildTreeNode);
        treeCache.set('', nodes);
        treeRootUl.replaceChildren(...nodes);
      },
      function (err) {
        // 根加载失败: 壳退出中止 (AbortError) 与旧壳一律静默 — 无可见陈旧
        // alert、无未处理拒绝; 当前壳清空根并把纯文本错误 (role=alert) 挂为
        // nav 内 treeRootUl 的兄弟 (绝不直接挂进 role=tree 的 ul), 链路
        // resolve; 重进/重试由新壳全新请求.
        if (err && err.name === 'AbortError') return;
        if (gen !== treeGen) return;
        treeCache.delete('');
        treeRootUl.replaceChildren();
        const errEl = document.createElement('div');
        errEl.className = 'cb-tree-error';
        errEl.setAttribute('role', 'alert');
        errEl.textContent = '无法加载目录树';
        treeNav.append(errEl);
      }
    );
  }

  /* ---------- 窄屏抽屉 (<64rem): off-canvas transform + 遮罩; 宽屏恒 no-op ---------- */

  function openTreeDrawer() {
    document.body.classList.add('cb-drawer-open');
    if (treeOpenBtn) {
      treeOpenBtn.setAttribute('aria-expanded', 'true');
    }
    // 焦点移入抽屉根树 (tabindex=0 单停靠点, aria-activedescendant 在根上
    // 公告活动后代), 键盘导航立即可用; nav 仅兜底 (根缺失时)
    const focusTarget = treeRootUl || treeNav;
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
  // 宽屏 resize 重钳制: 窄屏抽屉直接忽略 (resizer 隐藏, 宽度不生效); 宽屏按
  // 持久化偏好重钳制生效宽 (persist=false — 不写 localStorage, 偏好原样保留).
  function installTreeViewportResizeHandler() {
    treeViewportResizeHandler = function () {
      if (window.matchMedia(TREE_DRAWER_MQL).matches) return;
      setTreeWidth(treePreferredWidth, false);
    };
    window.addEventListener('resize', treeViewportResizeHandler);
  }

  function uninstallTreeViewportResizeHandler() {
    if (treeViewportResizeHandler) {
      window.removeEventListener('resize', treeViewportResizeHandler);
    }
    treeViewportResizeHandler = null;
  }
  // 焦点落点纯函数: 返回 'restore' | 'collapse' | 'drawer' | null.
  // - 折叠生效 (nextCollapsed): 树/调整器/抽屉唤起按钮内的焦点一律移交恢复
  //   按钮 (inert/隐藏元素不可聚焦, 必须先移出); 恢复按钮自身保持可见, 无需移交.
  // - 展开生效: 恢复按钮焦点落回折叠按钮 (宽屏 — 树列重新可见) 或抽屉唤起
  //   按钮 (窄屏 — 恢复按钮即将隐藏); 宽屏下抽屉唤起按钮焦点落回折叠按钮.
  // - 其余 (焦点在保持可见/可聚焦的元素, 或不在受影响控件内): null 不移交.
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

  // 树壳焦点分类: 树内 → 'tree' (折叠按钮已在树壳顶栏内, 归 'tree'), 调整器内
  // → 'resizer', 恢复按钮自身 → 'restore', 抽屉唤起按钮自身 → 'drawer', 其余
  // → null. 供 applyTreeCollapsed 与焦点追踪共用, 判定顺序先包含后 identity.
  function classifyTreeFocusTarget(target) {
    if (treeNav && treeNav.contains(target)) return 'tree';
    if (treeResizer && treeResizer.contains(target)) return 'resizer';
    if (treeRestoreBtn && target === treeRestoreBtn) return 'restore';
    if (treeOpenBtn && target === treeOpenBtn) return 'drawer';
    return null;
  }

  // 焦点追踪 (见 treeFocusKind 注释): focusin 捕获阶段记录/清空; 指针按下
  // 非树壳控件 (内容区等, 不可聚焦元素不触发 focusin) 时清空, 防止过期记录
  // 在后续断点过渡中误判.
  function onTreeFocusIn(event) {
    treeFocusKind = classifyTreeFocusTarget(event.target);
  }

  function onTreePointerDown(event) {
    if (!classifyTreeFocusTarget(event.target)) {
      treeFocusKind = null;
    }
  }

  // 焦点必须在任何 class/inert/hidden 变更之前捕获并分类: inert/隐藏元素
  // 不可聚焦, 一旦失能焦点即丢且无法再移交. 跨断点时焦点可能已被新视口 CSS
  // 静默丢弃 (见 treeFocusKind), 用追踪记录补判; 落点由纯函数计算, 与最终
  // 生效态互不纠缠.
  function applyTreeCollapsed() {
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
      // 折叠路径: 恢复按钮先解除 hidden → 折叠类 → 移交焦点 → 树/调整器
      // inert/hidden (display:none 不可聚焦, 恢复按钮必须先可见才能聚焦).
      if (treeRestoreBtn) treeRestoreBtn.hidden = false;
      document.body.classList.add('cb-tree-collapsed');
      if (focusTarget === 'restore' && treeRestoreBtn) {
        treeRestoreBtn.focus({ preventScroll: true });
        treeFocusKind = 'restore'; // 程序化移交不触发 focusin 的环境下直接记录
      }
      if (treeNav) {
        treeNav.inert = true;
        treeNav.setAttribute('aria-hidden', 'true');
      }
      if (treeResizer) treeResizer.hidden = true;
      return;
    }
    // 展开路径: 折叠类移除 → 树/调整器解除 inert/hidden → 落点聚焦 → 恢复
    // 按钮隐藏 (任何已绘制帧都不暴露恢复控件). 落点: 窄屏抽屉唤起按钮 (宽
    // →窄过渡, 恢复按钮即将隐藏); 宽屏折叠按钮 (树顶栏子控件, 已解除隐藏),
    // 折叠按钮缺失时回退调整器/树根 (nav 仅最后兜底) — 焦点绝不停留在隐藏/inert 元素.
    document.body.classList.remove('cb-tree-collapsed');
    if (treeNav) {
      treeNav.inert = false;
      treeNav.setAttribute('aria-hidden', 'false');
    }
    if (treeResizer) treeResizer.hidden = false;
    if (focusTarget === 'drawer') {
      // 收窄到抽屉且焦点在恢复按钮 (跨断点补判): 恢复按钮即将隐藏, 焦点移交
      // 可见的抽屉唤起按钮.
      if (treeOpenBtn) {
        treeOpenBtn.focus({ preventScroll: true });
        treeFocusKind = 'drawer';
      }
    } else if (focusTarget === 'restore' || focusTarget === 'collapse') {
      if (treeCollapseBtn) {
        treeCollapseBtn.focus({ preventScroll: true });
        treeFocusKind = 'tree'; // 折叠按钮在树顶栏内, 归 'tree' (与 classify 一致)
      } else if (treeResizer) {
        treeResizer.focus({ preventScroll: true });
        treeFocusKind = 'resizer';
      } else if (treeRootUl) {
        treeRootUl.focus({ preventScroll: true });
        treeFocusKind = 'tree';
      } else if (treeNav) {
        treeNav.focus({ preventScroll: true });
        treeFocusKind = 'tree';
      }
    }
    if (treeRestoreBtn) treeRestoreBtn.hidden = true;
  }

  // 折叠偏好: persist 只写两态 '1'/'0' (仅按钮交互写入; 窄屏抽屉交互不触碰
  // 该键), 随后立即重算生效态.
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

  // 断点感知: 收窄到抽屉时强制展开生效态 (偏好保留), 跨回宽屏时重放偏好.
  // 与 drawer mql 同断点同源; add/remove 走同一 handler 引用.
  function installTreeCollapseMql() {
    treeCollapseMql = window.matchMedia(TREE_DRAWER_MQL);
    treeCollapseMqlHandler = applyTreeCollapsed;
    if (treeCollapseMql.addEventListener) {
      treeCollapseMql.addEventListener('change', treeCollapseMqlHandler);
    } else if (treeCollapseMql.addListener) {
      treeCollapseMql.addListener(treeCollapseMqlHandler);
    }
  }

  function uninstallTreeCollapseMql() {
    if (treeCollapseMql && treeCollapseMqlHandler) {
      if (treeCollapseMql.removeEventListener) {
        treeCollapseMql.removeEventListener('change', treeCollapseMqlHandler);
      } else if (treeCollapseMql.removeListener) {
        treeCollapseMql.removeListener(treeCollapseMqlHandler);
      }
    }
    treeCollapseMql = null;
    treeCollapseMqlHandler = null;
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
    if (!paneNavGuard.isCurrent(gen)) return; // 陈旧导航: 树状态不跟进 (先于任何 mutation)
    const encoded = currentPathFromLocation();
    if (encoded === null || !treeRootUl) return;
    const segments = encoded === '' ? [] : encoded.split('/');
    clearTreeCurrent();
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
      const registeredDisposer = disposer;
      registerPaneDisposable(function () {
        if (paneSpyDisposer === registeredDisposer) {
          paneSpyDisposer = null;
        }
        registeredDisposer();
      });
    }
  }

  function enterTreeMode() {
    // 新壳开始: 递增使旧壳在途响应全部失效 (exit 已递增; 此处确保首入与连续
    // 进出间的任何在途响应都追不上新壳), 进入链 generation 即此值.
    treeGen += 1;
    const enterGen = treeGen;
    // 本壳树请求 (root/子项) 的取消信号: 先于 buildTreeShell 的根请求与
    // reveal/persist 子项请求创建; 退出时 abort+置空 (在途以 AbortError 收尾).
    treeAbort = new AbortController();
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
    // 折叠偏好: 仅 '1' 视为折叠 (缺省/损坏 → 展开). 先装 mql 再应用 — apply
    // 以当前窄屏状态为准 (窄屏恒展开, 偏好保留到跨回宽屏重放).
    treeCollapsePreferred = false;
    try {
      treeCollapsePreferred = localStorage.getItem(TREE_COLLAPSED_KEY) === '1';
    } catch (err) {
      /* 持久化读取失败: 默认展开 */
    }
    installTreeCollapseMql();
    applyTreeCollapsed();
    // 树壳焦点追踪: 跨断点过渡需要最近一次树壳焦点记录 (见 treeFocusKind),
    // 随壳安装/卸载.
    document.addEventListener('focusin', onTreeFocusIn, true);
    document.addEventListener('pointerdown', onTreePointerDown, true);
    // 宽屏视口 resize 重钳制: 宽度状态就绪后安装一次 (生效宽随 50vw 上限浮动)
    installTreeViewportResizeHandler();
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
      // 旧进入链 (退出/重进后): 新壳已接管, 不得触碰新全局
      if (enterGen !== treeGen) return;
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
        if (enterGen !== treeGen) return;
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
            // 进入链所有权随加载传递: 退出/重进后旧链响应不得提交
            return loadTreeChildren(node, nodePathV, { enterGen: enterGen }).then(function () {
              // 旧进入链续接: 退出/重进后不得继续遍历新壳 DOM
              if (enterGen !== treeGen) return Promise.resolve();
              ensureTreeChildrenInstalled(node, nodePathV);
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
      // 旧进入链 (退出/重进后): 不得恢复选中
      if (enterGen !== treeGen) return;
      // 启动链的选中恢复只属于初始导航 generation: 用户已导航走则不覆盖
      if (paneInitialGen === null || paneNavGuard.isCurrent(paneInitialGen)) {
        restoreTreeSelection();
      }
    });
  }

  function exitTreeMode() {
    const hadPane = pane !== null;
    treeGen += 1; // 使全部在途树响应失效 (旧壳 continuation 不得触碰新壳)
    if (treeAbort) {
      treeAbort.abort(); // 中止本壳在途树请求 (root/子项): AbortError 由失败
      treeAbort = null; // 处理器静默吞掉, 绝不产生可见陈旧 alert/未处理拒绝
    }
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
    uninstallTreeViewportResizeHandler();
    uninstallTreeCollapseMql();
    document.removeEventListener('focusin', onTreeFocusIn, true);
    document.removeEventListener('pointerdown', onTreePointerDown, true);
    treeFocusKind = null; // 焦点追踪随壳销毁, 不残留到下一次进入
    document.body.classList.remove('cb-tree-collapsed');
    // 在途拖拽手势先安全复位: 移除 resizer 会隐式释放捕获并触发
    // lostpointercapture — owner 此刻已复位, 幂等收尾重入直接返回, 不重复清理.
    if (treeResizePointerId !== null) {
      treeResizePointerId = null;
      document.body.classList.remove('cb-tree-resizing');
    }
    // 树宽调整器与 CSS 变量随壳销毁: 残留变量会污染再进入时的默认列 (变量
    // 只在树模式被 JS 重新写入, 但避免 dir 宿主残留样式差异).
    if (treeResizer) {
      treeResizer.remove();
      treeResizer = null;
    }
    treeCollapseBtn = null; // 折叠按钮在树壳顶栏内, 随壳销毁; 引用必须清空
    document.body.style.removeProperty('--cb-tree-width');
    document.body.classList.remove('cb-tree-resizing');
    if (treeScrim) treeScrim.remove();
    treeScrim = null;
    if (treeOpenBtn) treeOpenBtn.remove();
    treeOpenBtn = null;
    if (treeRestoreBtn) treeRestoreBtn.remove();
    treeRestoreBtn = null;
    // 模式切换按钮跨模式复用: 树壳销毁 (treeNav.remove) 前必须迁回目录工具
    // 栏行 — append 移动节点, 按钮随树壳销毁即永久丢失.
    if (modeToggleBtn && modeToolbar) {
      modeToolbar.append(modeToggleBtn);
    }
    if (treeNav) treeNav.remove();
    treeNav = null;
    treeRootUl = null;
    treeCache.clear();
    treePendingLoads.clear(); // 在途加载与意图随壳销毁 (旧壳响应已由 treeGen 作废)
    treeExpansionIntents.clear(); // 意图记录随壳销毁 (版本计数保留: 跨壳单调)
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
    // 树模式隐藏目录工具栏行 (搜索随树内搜索让位); 退出恢复 (按钮已迁回).
    if (modeToolbar) {
      modeToolbar.hidden = treeMode;
    }
  }

  // 目录模式左缘工具栏行: 搜索表单 + 模式切换按钮 (仅树能力页; 搜索页绝不
  // 创建 — 顶栏搜索表单保持原位). 目录页把顶栏搜索表单移动 (非克隆) 进本行;
  // 文档页无顶栏表单, 复用树搜索结构 (独立 id, 与树内搜索不撞). 树模式隐藏
  // 本行, 按钮移入树顶栏; 退出恢复.
  function buildModeToolbar() {
    if (!isTreeCapablePage() || $('.cb-mode-toolbar')) return;
    const toolbar = document.createElement('div');
    toolbar.className = 'cb-mode-toolbar';
    const topbarSearch = document.querySelector('header.cb-topbar form.cb-search');
    if (topbarSearch) {
      toolbar.append(topbarSearch);
    } else {
      toolbar.append(treeSearchForm('cb-mode-search-q'));
    }
    modeToggleBtn = document.createElement('button');
    modeToggleBtn.type = 'button';
    modeToggleBtn.className = 'cb-mode-toggle';
    modeToggleBtn.addEventListener('click', function () {
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
    toolbar.append(modeToggleBtn);
    modeToolbar = toolbar;
    document.body.prepend(toolbar);
  }

  // 树键范围纯函数 (DOM 无关 seam): 返回 'escape-search' | 'focus-search' |
  // 'tree' | null.
  // - Escape 只在搜索框聚焦 (blur/收抽屉) 或树根聚焦 (清选中/收抽屉) 时处理,
  //   其余焦点一律放行原生;
  // - '/' 是刻意全局快捷: 非可编辑/交互即聚焦树搜索; 搜索框内 (可编辑) 原样
  //   输入 '/' 字符;
  // - Arrow/Enter/Backspace/Page 仅树根聚焦时属于树: pane/链接/按钮/切换/
  //   body 聚焦一律 null — 不阻断原生滚动/激活;
  // - 可编辑守卫晚于 Escape (搜索框聚焦的 Escape 优先), 早于 '/' 与导航键.
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
    // 恢复按钮 (body 直系子级, 不在 resizer 内): Enter/Space 只触发恢复, 不得
    // 落入全局树键; 折叠按钮在树壳顶栏内, 其按键经 treeKeyScope 决策 (焦点不在
    // 树根/搜索框时返回 null, 原生 click 照常触发折叠).
    if (event.target.closest('.cb-tree-restore')) return;
    const searchInput = $('#cb-tree-search-q');
    const scope = treeKeyScope(
      event.key,
      isEditableTarget(event.target),
      !!treeRootUl && document.activeElement === treeRootUl,
      document.activeElement === searchInput
    );
    if (scope === null) return; // 非树范围按键: 不取消默认, 放行原生行为
    if (scope === 'escape-search') {
      // 窄屏抽屉: 先收抽屉 (closeTreeDrawer 把焦点归还唤起按钮), 再失焦 —
      // 顺序颠倒会让 blur 先把焦点丢给 body, 抽屉关闭后的焦点归还落空;
      // 宽屏 (抽屉未开) 仅失焦.
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
    // scope === 'tree': Escape 清选中并收抽屉; 其余为树导航键 (rootFocused
    // 已由 helper 保证)
    if (event.key === 'Escape') {
      clearTreeSelection();
      closeTreeDrawer();
      event.preventDefault();
      return;
    }
    switch (event.key) {
      case 'PageUp':
        // Page 滚动只响应聚焦的树根: 根 ul 存在且为事件目标或活动元素; 焦点在
        // pane/链接/切换按钮/body 时放行原生滚动 (不滚动树列、不阻断默认).
        if (
          treeRootUl &&
          (event.target === treeRootUl || document.activeElement === treeRootUl)
        ) {
          if (treeNav) {
            treeNav.scrollBy({ top: -Math.round(treeNav.clientHeight * 0.8) });
          }
          event.preventDefault();
        }
        break;
      case 'PageDown':
        if (
          treeRootUl &&
          (event.target === treeRootUl || document.activeElement === treeRootUl)
        ) {
          if (treeNav) {
            treeNav.scrollBy({ top: Math.round(treeNav.clientHeight * 0.8) });
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
        // → 只展开 (契约: → 展开 / ← 收起); 已展开则 no-op. 显式激活发布显式
        // 展开意图 (覆盖既有收起意图 — 最新用户动作权威, 版本化), 在途响应
        // 终态跟随最新记录.
        if (treeSelectedEl && isExpandableDir(treeSelectedEl)) {
          const ul = treeSelectedEl.querySelector(':scope > ul.cb-tree-children');
          if (!ul || ul.hidden) {
            const path = encodedPathOf(treeSelectedEl);
            if (path !== null) {
              publishTreeExpansionIntent(path, true, true);
              loadTreeChildren(treeSelectedEl, path);
            }
          }
          event.preventDefault();
        }
        break;
      case 'ArrowLeft': {
        if (treeSelectedEl) {
          const ul = treeSelectedEl.querySelector(':scope > ul.cb-tree-children');
          if (isDirNode(treeSelectedEl) && ul && !ul.hidden) {
            // 可见 ul 即已加载 (无在途): 直接改 DOM, 意图无消费方, 不留残留
            // (陈旧意图会挡住后续 reveal 的缓存命中展开).
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
      default:
        break;
    }
  }

  function init() {
    const tokenMeta = document.querySelector(
      'meta[name="cb-native-open-token"]'
    );
    nativeToken = tokenMeta ? tokenMeta.getAttribute('content') : null;
    buildModeToolbar();
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
      canStartTreeResize: canStartTreeResize,
      ownsTreeResize: ownsTreeResize,
      collapseFocusTarget: collapseFocusTarget,
      treeKeyScope: treeKeyScope,
      treeNextExplicitIntent: treeNextExplicitIntent,
      publishTreeExpansionIntent: publishTreeExpansionIntent,
      treeLoadMayCommit: treeLoadMayCommit,
    };
  }
})();
