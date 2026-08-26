# Tree Reading Layout Refinement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the overlapping TOC/tree layout with a true reading workspace: resizable and manually collapsible tree, non-overlapping TOC column, hidden tree scrollbar, and single-click file opening.

**Architecture:** Keep the existing server HTML and progressively enhance it. A shared `installDocLayout()` seam moves the existing TOC node and remaining document nodes into a TOC-first DOM wrapper with named grid areas; tree width/collapse state are CSS-variable-driven and persisted in localStorage. SPA fragment installation retains strict target binding through a segment-wise canonical transport comparison key.

**Tech Stack:** Vanilla JavaScript IIFE, CSS Grid/Pointer Events/IntersectionObserver, Rust source-invariant tests (`cargo test`), existing raw-CDP Chromium acceptance harness.

---

## File Structure

- Modify `assets/chapbook-browser.js`
  - canonical transport comparison key
  - document layout installation seam
  - tree resizer/collapse lifecycle
  - tree click/double-click and ARIA behavior
- Modify `assets/chapbook-doc.css`
  - true document-content/TOC grid
  - resizer/gutter, hidden scrollbar and collapsed-tree states
  - delete obsolete fixed/float TOC rules
- Modify `src/assets.rs`
  - source invariants for exported JS seams and action-identity safety
- Modify `src/render.rs`
  - CSS invariants for true grid layout and absence of float/fixed TOC rules
- Update `docs/superpowers/specs/2026-08-26-tree-reading-layout-design.md` only if implementation discovers a specification contradiction

No server route, filesystem traversal, native-open, search, sorting or document-rendering changes are required.

---

### Task 1: Canonical Fragment Transport Binding

**Files:**
- Modify: `src/assets.rs:118-257`
- Modify: `assets/chapbook-browser.js:18-120, 1195-1240, CommonJS export block`

- [ ] **Step 1: Add a failing source-invariant test**

Add to `src/assets.rs` inside `mod tests`:

```rust
#[test]
fn browser_js_canonical_transport_binding_seam_is_locked() {
    let js = BROWSER_JS;
    for seam in [
        "function canonicalTransportKey(",
        "canonicalTransportKey(encodedPath)",
        "canonicalTransportKey(wrapperPath)",
    ] {
        assert!(js.contains(seam), "missing transport seam: {seam}");
    }
    let start = js
        .find("function canonicalTransportKey(")
        .expect("transport-key function");
    let end = js[start..]
        .find("function actionUrl(")
        .map(|n| start + n)
        .expect("actionUrl after transport key");
    let body = &js[start..end];
    assert!(!body.contains("decodeURIComponent("), "action transport must never decode");
    assert!(body.contains("byte === 0x2f"), "encoded slash must be rejected");
    assert!(js.contains("canonicalTransportKey: canonicalTransportKey"));
}
```

- [ ] **Step 2: Run the targeted test and verify RED**

Run:

```bash
cargo test assets::tests::browser_js_canonical_transport_binding_seam_is_locked -- --exact
```

Expected: FAIL with `missing transport seam: function canonicalTransportKey(`.

- [ ] **Step 3: Add the pure canonical-key function and export it**

Insert in the pure-function section before `actionUrl()` in `assets/chapbook-browser.js`:

```js
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
          if (byte === 0x2f) return null; // never decode a separator
          const ascii = byte < 0x80 ? String.fromCharCode(byte) : null;
          segment += ascii && TRANSPORT_LITERAL_ASCII.test(ascii)
            ? ascii
            : hexByte(byte);
          i += 3;
          continue;
        }
        const scalar = String.fromCodePoint(rawSegment.codePointAt(i));
        const bytes = new TextEncoder().encode(scalar);
        if (bytes.length === 1 && TRANSPORT_LITERAL_ASCII.test(scalar)) {
          segment += scalar;
        } else {
          bytes.forEach(function (byte) {
            segment += hexByte(byte);
          });
        }
        i += scalar.length;
      }
      if (segment === '.' || segment === '..') return null;
      out.push(segment);
    }
    return out.join('/');
  }
```

Expose it in the existing CommonJS guard:

```js
      canonicalTransportKey: canonicalTransportKey,
```

- [ ] **Step 4: Bind the SPA target before DOM/history commit**

In `commitPane()`, replace “accept any valid wrapper path” with exact key comparison:

```js
    const wrapperPath =
      wrapper && typeof wrapper.dataset.nativePathEncoded === 'string'
        ? wrapper.dataset.nativePathEncoded
        : null;
    const requestedKey = canonicalTransportKey(encodedPath);
    const wrapperKey = canonicalTransportKey(wrapperPath);
    const canonicalUrl =
      requestedKey !== null &&
      wrapperKey !== null &&
      requestedKey === wrapperKey
        ? actionUrl(wrapperPath, 'full')
        : null;
```

Do not change `actionUrl()` or introduce another `decodeURIComponent()`.

- [ ] **Step 5: Run source test and the transport matrix**

Run:

```bash
cargo test assets::tests::browser_js_canonical_transport_binding_seam_is_locked -- --exact
node - <<'NODE'
const { canonicalTransportKey: k } = require('./assets/chapbook-browser.js');
const cases = [
  ['%41GENTS.md', 'AGENTS.md'],
  ['%2b.txt', '%2B.txt'],
  ['中.md', '%E4%B8%AD.md'],
  ['%E4%b8%AD.md', '%E4%B8%AD.md'],
  ['a+b.md', 'a%2Bb.md'],
  ['a%25b.md', 'a%25b.md'],
  ['a\\b.md', 'a%5Cb.md'],
  ['a%5cb.md', 'a%5Cb.md'],
  ['a%2Fb.md', null],
  ['%2e%2e', null],
  ['bad%', null],
];
for (const [input, expected] of cases) {
  const actual = k(input);
  if (actual !== expected) throw new Error(`${input}: ${actual} !== ${expected}`);
}
console.log('transport matrix PASS');
NODE
```

Expected: targeted Rust test PASS; Node prints `transport matrix PASS`.

- [ ] **Step 6: Commit**

```bash
git add src/assets.rs assets/chapbook-browser.js
git commit -m "fix(tree): bind fragments with canonical transport keys"
```

---

### Task 2: True Document and TOC Columns

**Files:**
- Modify: `src/assets.rs:158-257`
- Modify: `src/render.rs:829-955`
- Modify: `assets/chapbook-browser.js:1080-1460, 1620-1660`
- Modify: `assets/chapbook-doc.css:28-176, 226-255, 400-475`

- [ ] **Step 1: Add failing JS/CSS source invariants**

Add to `src/assets.rs`:

```rust
#[test]
fn browser_js_document_layout_seam_is_locked() {
    let js = BROWSER_JS;
    for seam in [
        "function installDocLayout(",
        "cb-doc-layout",
        "cb-doc-toc",
        "cb-doc-content",
        "installDocLayout(wrapper.querySelector('.cb-doc'))",
    ] {
        assert!(js.contains(seam), "missing document-layout seam: {seam}");
    }
    assert!(js.contains("tocHost.append(tocNav)"), "TOC node must move, not clone");
}
```

Extend `render::tests::doc_css_scopes_layout_and_content`:

```rust
assert!(css.contains("grid-template-areas: \"content toc\""));
assert!(css.contains("grid-area: content"));
assert!(css.contains("grid-area: toc"));
assert!(!css.contains("float: right"), "TOC must not float over document text");
assert!(!css.contains(".cb-doc-page #TOC {\n    position: fixed"));
```

- [ ] **Step 2: Run tests and verify RED**

```bash
cargo test assets::tests::browser_js_document_layout_seam_is_locked -- --exact
cargo test render::tests::doc_css_scopes_layout_and_content -- --exact
```

Expected: both fail on missing layout seam / current float+fixed rail CSS.

- [ ] **Step 3: Implement the shared idempotent DOM layout**

Add near the TOC helpers in `assets/chapbook-browser.js`:

```js
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

    tocHost.append(tocNav); // preserve the original node + slug/href pairing
    originalNodes.forEach(function (node) {
      if (node !== tocNav) content.append(node);
    });
    layout.append(tocHost, content); // TOC-first DOM for a11y/narrow screens
    docEl.replaceChildren(layout);
    return layout;
  }
```

- [ ] **Step 4: Wire full-page and pane lifecycles**

Before full-page spy installation:

```js
  function initFullPageTocSpy() {
    if (!document.body.classList.contains('cb-doc-page')) return;
    const docEl = document.querySelector('.cb-doc');
    installDocLayout(docEl);
    const tocNav = docEl ? tocNavIn(docEl) : null;
    if (!tocNav) return;
    fullPageSpyDisposer = installTocSpy(docEl, tocNav, null);
  }
```

After `pane.replaceChildren(wrapper)` in `commitPane()`:

```js
    const docEl = wrapper.matches('.cb-doc')
      ? wrapper
      : wrapper.querySelector('.cb-doc');
    installDocLayout(docEl);
    installPaneTocSpy(wrapper);
```

Keep `teardownPane()` before replacement so the prior observer disconnects before the old layout is removed.

- [ ] **Step 5: Replace fixed/float rail CSS with named grid areas**

Delete the old wide full-page `position: fixed`, body margin reservation and pane `float: right`/floated-header rules. Add:

```css
.cb-doc-layout {
  min-width: 0;
}
.cb-doc-content {
  min-width: 0;
}
.cb-doc-toc {
  min-width: 0;
}

@media screen and (min-width: 75rem) {
  .cb-doc-layout {
    display: grid;
    grid-template-areas: "content toc";
    grid-template-columns: minmax(0, 1fr) 240px;
    gap: 20px;
    align-items: start;
  }
  .cb-doc-content { grid-area: content; }
  .cb-doc-toc {
    grid-area: toc;
    position: sticky;
    top: 0;
    max-height: calc(100vh - 2rem);
    overflow-y: auto;
  }
}

@media screen and (max-width: 74.999rem) {
  .cb-doc-layout {
    display: grid;
    grid-template-areas: "toc" "content";
    grid-template-columns: minmax(0, 1fr);
  }
  .cb-doc-toc { grid-area: toc; }
  .cb-doc-content { grid-area: content; }
}
```

- [ ] **Step 6: Run focused tests and a Chromium layout smoke**

```bash
cargo test assets::tests::browser_js_document_layout_seam_is_locked -- --exact
cargo test render::tests::doc_css_scopes_layout_and_content -- --exact
node --check assets/chapbook-browser.js
```

Chromium expectations at 1568px:

- `.cb-doc-layout` computed grid columns end in `240px`.
- `.cb-doc-content` and `.cb-doc-toc` rectangles do not overlap.
- scrolling to the final paragraph never places text below the TOC rectangle.
- at 1000px the TOC rectangle appears before the content rectangle vertically.

- [ ] **Step 7: Commit**

```bash
git add src/assets.rs src/render.rs assets/chapbook-browser.js assets/chapbook-doc.css
git commit -m "feat(doc): render TOC as a true sibling column"
```

---

### Task 3: Resizable Tree, Hidden Scrollbar and Pane Gap

**Files:**
- Modify: `src/assets.rs:158-257`
- Modify: `assets/chapbook-browser.js:600-1065, 1460-1565`
- Modify: `assets/chapbook-doc.css:195-325, 450-500`

- [ ] **Step 1: Add failing source invariants**

Add to `src/assets.rs`:

```rust
#[test]
fn browser_js_tree_resize_seams_are_locked() {
    let js = BROWSER_JS;
    for seam in [
        "TREE_WIDTH_KEY = 'cb-tree-width'",
        "function clampTreeWidth(",
        "function setTreeWidth(",
        "role', 'separator'",
        "setPointerCapture",
        "aria-valuenow",
    ] {
        assert!(js.contains(seam), "missing resize seam: {seam}");
    }
}
```

- [ ] **Step 2: Run test and verify RED**

```bash
cargo test assets::tests::browser_js_tree_resize_seams_are_locked -- --exact
```

Expected: FAIL on missing `cb-tree-width` seam.

- [ ] **Step 3: Add tree-width state and accessible resizer**

Add to the tree state:

```js
  const TREE_WIDTH_KEY = 'cb-tree-width';
  const TREE_DEFAULT_WIDTH = 238;
  const TREE_MIN_WIDTH = 160;
  const TREE_MAX_WIDTH = 480;
  let treeWidth = TREE_DEFAULT_WIDTH;
  let treeResizer = null;

  function treeWidthMax() {
    return Math.min(TREE_MAX_WIDTH, Math.floor(window.innerWidth * 0.5));
  }

  function clampTreeWidth(width) {
    return Math.max(TREE_MIN_WIDTH, Math.min(treeWidthMax(), Math.round(width)));
  }

  function setTreeWidth(width, persist) {
    treeWidth = clampTreeWidth(width);
    document.body.style.setProperty('--cb-tree-width', treeWidth + 'px');
    if (treeResizer) {
      treeResizer.setAttribute('aria-valuemax', String(treeWidthMax()));
      treeResizer.setAttribute('aria-valuenow', String(treeWidth));
    }
    if (persist) {
      try { localStorage.setItem(TREE_WIDTH_KEY, String(treeWidth)); } catch (err) {}
    }
  }
```
Create the separator in `buildTreeShell()` after the tree nav:

```js
    treeResizer = document.createElement('div');
    treeResizer.className = 'cb-tree-resizer';
    treeResizer.tabIndex = 0;
    treeResizer.setAttribute('role', 'separator');
    treeResizer.setAttribute('aria-orientation', 'vertical');
    treeResizer.setAttribute('aria-valuemin', String(TREE_MIN_WIDTH));
    treeResizer.setAttribute('aria-valuemax', String(treeWidthMax()));
    document.body.append(treeResizer);
```

Install pointer handlers with pointer capture:

```js
    let resizeStartX = 0;
    let resizeStartWidth = 0;
    treeResizer.addEventListener('pointerdown', function (event) {
      // 折叠按钮是 separator 子节点: click.stopPropagation() 太晚,
      // 必须在 pointerdown 阶段阻止它启动 resize/pointer capture.
      if (event.target.closest('.cb-tree-collapse')) return;
      resizeStartX = event.clientX;
      resizeStartWidth = treeWidth;
      treeResizer.setPointerCapture(event.pointerId);
      document.body.classList.add('cb-tree-resizing');
    });
    treeResizer.addEventListener('pointermove', function (event) {
      if (!treeResizer.hasPointerCapture(event.pointerId)) return;
      setTreeWidth(resizeStartWidth + event.clientX - resizeStartX, false);
    });
    function finishResize(event) {
      if (treeResizer.hasPointerCapture(event.pointerId)) {
        treeResizer.releasePointerCapture(event.pointerId);
      }
      document.body.classList.remove('cb-tree-resizing');
      setTreeWidth(treeWidth, true);
    }
    treeResizer.addEventListener('pointerup', finishResize);
    treeResizer.addEventListener('pointercancel', finishResize);
```

Keyboard handler:

```js
    treeResizer.addEventListener('keydown', function (event) {
      let next = null;
      if (event.key === 'ArrowLeft') next = treeWidth - 16;
      if (event.key === 'ArrowRight') next = treeWidth + 16;
      if (event.key === 'Home') next = TREE_MIN_WIDTH;
      if (event.key === 'End') next = treeWidthMax();
      if (next === null) return;
      event.preventDefault();
      setTreeWidth(next, true);
    });
```

- [ ] **Step 4: Load and clean up the persisted width**

In `enterTreeMode()`:

```js
    let savedWidth = TREE_DEFAULT_WIDTH;
    try { savedWidth = Number(localStorage.getItem(TREE_WIDTH_KEY)) || TREE_DEFAULT_WIDTH; } catch (err) {}
    setTreeWidth(savedWidth, false);
```

In `exitTreeMode()` remove and null `treeResizer`; remove `--cb-tree-width` and `cb-tree-resizing`.

- [ ] **Step 5: Update CSS grid/gap and hide scrollbar chrome**

Replace the wide tree grid with:

```css
body.cb-tree-mode {
  grid-template-columns: var(--cb-tree-width, 238px) 8px minmax(0, 1fr);
}
body.cb-tree-mode .cb-tree { grid-column: 1; }
body.cb-tree-mode .cb-tree-resizer {
  grid-column: 2;
  position: relative;
  cursor: col-resize;
  touch-action: none;
}
body.cb-tree-mode .cb-tree-resizer::before {
  content: "";
  position: absolute;
  left: 3px;
  top: 0;
  bottom: 0;
  width: 2px;
  background: var(--cb-border);
}
body.cb-tree-mode > *:not(.cb-tree):not(.cb-tree-resizer):not(.cb-mode-toggle) {
  grid-column: 3;
}
body.cb-tree-mode .cb-tree {
  overflow-y: auto;
  scrollbar-width: none;
}
body.cb-tree-mode .cb-tree::-webkit-scrollbar { display: none; }
body.cb-tree-resizing { cursor: col-resize; user-select: none; }
```

Inside the existing `<64rem` drawer media query, hide `.cb-tree-resizer`, keep the single-column drawer layout and reset the content column selector. The `8px` resizer column itself is the entire tree/content visual gap; do not add a second margin.

- [ ] **Step 6: Verify and commit**

Run:

```bash
cargo test assets::tests::browser_js_tree_resize_seams_are_locked -- --exact
node --check assets/chapbook-browser.js
cargo test
```

Chromium: drag to 160/480 boundaries, release outside handle, reload, use keyboard separator controls, and verify wheel/PageDown tree scrolling with no visible scrollbar.

Commit:

```bash
git add src/assets.rs assets/chapbook-browser.js assets/chapbook-doc.css
git commit -m "feat(tree): add resizable scrollbar-free sidebar"
```

---

### Task 4: Manual Wide-Screen Tree Collapse

**Files:**
- Modify: `src/assets.rs:158-257`
- Modify: `assets/chapbook-browser.js:600-1065, 1460-1565`
- Modify: `assets/chapbook-doc.css:195-410`

- [ ] **Step 1: Add a failing source-invariant test**
```rust
#[test]
fn browser_js_tree_collapse_seams_are_locked() {
    let js = BROWSER_JS;
    for seam in [
        "TREE_COLLAPSED_KEY = 'cb-tree-collapsed'",
        "treeCollapsePreferred",
        "function applyTreeCollapsed(",
        "function installTreeCollapseMql(",
        "cb-tree-collapsed",
        "cb-tree-restore",
    ] {
        assert!(js.contains(seam), "missing collapse seam: {seam}");
    }
}
```

- [ ] **Step 2: Run test and verify RED**

```bash
cargo test assets::tests::browser_js_tree_collapse_seams_are_locked -- --exact
```

- [ ] **Step 3: Add state and controls**
```js
  const TREE_COLLAPSED_KEY = 'cb-tree-collapsed';
  let treeCollapsePreferred = false; // persisted wide-screen preference
  let treeCollapsed = false; // currently effective layout state
  let treeRestoreBtn = null;
  let treeCollapseMql = null;
  let treeCollapseMqlHandler = null;

  function applyTreeCollapsed() {
    const narrow = treeCollapseMql
      ? treeCollapseMql.matches
      : window.matchMedia('(width < 64rem)').matches;
    treeCollapsed = treeCollapsePreferred && !narrow;
    document.body.classList.toggle('cb-tree-collapsed', treeCollapsed);
    if (treeNav) {
      treeNav.inert = treeCollapsed;
      treeNav.setAttribute('aria-hidden', treeCollapsed ? 'true' : 'false');
    }
    if (treeResizer) treeResizer.hidden = treeCollapsed;
    if (treeRestoreBtn) treeRestoreBtn.hidden = !treeCollapsed;
    if (
      treeCollapsed &&
      treeRestoreBtn &&
      treeNav &&
      treeNav.contains(document.activeElement)
    ) {
      treeRestoreBtn.focus({ preventScroll: true });
    }
  }

  function setTreeCollapsePreference(preferred, persist) {
    treeCollapsePreferred = preferred;
    if (persist) {
      try {
        localStorage.setItem(TREE_COLLAPSED_KEY, preferred ? '1' : '0');
      } catch (err) {}
    }
    applyTreeCollapsed();
  }

  function installTreeCollapseMql() {
    treeCollapseMql = window.matchMedia('(width < 64rem)');
    treeCollapseMqlHandler = applyTreeCollapsed;
    if (treeCollapseMql.addEventListener) {
      treeCollapseMql.addEventListener('change', treeCollapseMqlHandler);
    } else {
      treeCollapseMql.addListener(treeCollapseMqlHandler);
    }
  }

  function uninstallTreeCollapseMql() {
    if (treeCollapseMql && treeCollapseMqlHandler) {
      if (treeCollapseMql.removeEventListener) {
        treeCollapseMql.removeEventListener('change', treeCollapseMqlHandler);
      } else {
        treeCollapseMql.removeListener(treeCollapseMqlHandler);
      }
    }
    treeCollapseMql = null;
    treeCollapseMqlHandler = null;
  }
```

Add a collapse button inside the resizer and a left-edge restore button:

```js
    const collapseBtn = document.createElement('button');
    collapseBtn.type = 'button';
    collapseBtn.className = 'cb-tree-collapse';
    collapseBtn.title = '隐藏目录树';
    collapseBtn.setAttribute('aria-label', '隐藏目录树');
    collapseBtn.textContent = '‹';
    collapseBtn.addEventListener('click', function (event) {
      event.stopPropagation();
      setTreeCollapsePreference(true, true);
    });
    treeResizer.append(collapseBtn);

    treeRestoreBtn = document.createElement('button');
    treeRestoreBtn.type = 'button';
    treeRestoreBtn.className = 'cb-tree-restore';
    treeRestoreBtn.textContent = '›';
    treeRestoreBtn.title = '显示目录树';
    treeRestoreBtn.setAttribute('aria-label', '显示目录树');
    treeRestoreBtn.addEventListener('click', function () {
      setTreeCollapsePreference(false, true);
    });
    document.body.append(treeRestoreBtn);
```

In `enterTreeMode()`, load `treeCollapsePreferred`, install the media-change listener, then call `applyTreeCollapsed()`. In `exitTreeMode()`, uninstall the listener, remove controls and clear the effective class without overwriting the persisted preference.

- [ ] **Step 4: Add collapsed CSS**

```css
body.cb-tree-mode.cb-tree-collapsed {
  grid-template-columns: 0 0 minmax(0, 1fr);
}
body.cb-tree-mode.cb-tree-collapsed .cb-tree,
body.cb-tree-mode.cb-tree-collapsed .cb-tree-resizer {
  visibility: hidden;
  pointer-events: none;
}
.cb-tree-restore {
  display: none;
  position: fixed;
  left: 0;
  top: 50%;
  z-index: 20;
}
body.cb-tree-mode.cb-tree-collapsed .cb-tree-restore { display: block; }
@media (width < 64rem) {
  body.cb-tree-mode.cb-tree-collapsed {
    grid-template-columns: minmax(0, 1fr); /* drawer always wins while narrow */
  }
  body.cb-tree-mode .cb-tree-restore,
  body.cb-tree-mode .cb-tree-collapse { display: none; }
}
```

- [ ] **Step 5: Verify and commit**

Chromium: collapse/restore, reload persistence, saved width restoration, focus never remains in hidden tree, collapse button pointerdown never starts resize or writes `cb-tree-width`, and the full transition matrix: wide collapsed → narrow drawer opens normally → wide reapplies the persisted collapsed preference.

```bash
cargo test assets::tests::browser_js_tree_collapse_seams_are_locked -- --exact
cargo test
node --check assets/chapbook-browser.js
git add src/assets.rs assets/chapbook-browser.js assets/chapbook-doc.css
git commit -m "feat(tree): add persistent manual reading collapse"
```

---

### Task 5: Single-Click Tree Actions and Keyboard-Focusable Tree

**Files:**
- Modify: `src/assets.rs:158-257`
- Modify: `assets/chapbook-browser.js:448-520, 678-850, 1240-1360`

- [ ] **Step 1: Add failing source invariants**

```rust
#[test]
fn browser_js_tree_click_and_aria_contract_is_locked() {
    let js = BROWSER_JS;
    assert!(js.contains("if (event.detail > 1) return"));
    assert!(js.contains("if (isDirNode(li))"));
    assert!(js.contains("navigateTreeNode(li)"));
    assert!(js.contains("treeRootUl.setAttribute('role', 'tree')"));
    assert!(js.contains("li.setAttribute('role', 'treeitem')"));
    assert!(js.contains("ul.setAttribute('role', 'group')"));
    assert!(js.contains("treeRootUl.tabIndex = 0"));
}
```

- [ ] **Step 2: Run test and verify RED**

```bash
cargo test assets::tests::browser_js_tree_click_and_aria_contract_is_locked -- --exact
```

- [ ] **Step 3: Add tree semantics**

In `buildTreeShell()`:

```js
    treeNav.removeAttribute('tabindex'); // remove current programmatic-only tabIndex=-1
    treeNav.setAttribute('aria-label', '目录树');
    treeRootUl.setAttribute('role', 'tree');
    treeRootUl.tabIndex = 0;
```
In `buildTreeNode()`:

```js
    li.setAttribute('role', 'treeitem');
    if (isDir) li.setAttribute('aria-expanded', 'false');
```

In `childrenUlFor()` set `role="group"`; in `setTreeNodeExpanded()` update `aria-expanded`.

- [ ] **Step 4: Implement exact single-click behavior and disable tree double-click**

In the tree-row branch of `onDocumentClick()`:

```js
    const treeRow = event.target.closest('.cb-tree-row');
    if (treeRow) {
      if (event.detail > 1) return; // first click owns the action
      const li = treeRow.closest('li.cb-tree-node');
      if (!li) return;
      selectTreeNode(li);
      if (isDirNode(li)) {
        toggleTreeNode(li);
      } else {
        navigateTreeNode(li); // SPA when pane exists; full navigation fallback
      }
      closeTreeDrawer();
      return;
    }
```

At the top of the tree branch in `onDocumentDblclick()`:

```js
    if (event.target.closest('.cb-tree-row')) {
      event.preventDefault();
      return; // tree double-click is intentionally a no-op
    }
```

- [ ] **Step 5: Add discoverable keyboard scrolling**

Keep existing Arrow selection behavior. When the focused tree receives PageUp/PageDown, scroll its viewport without changing action identity:

```js
      case 'PageUp':
        if (treeNav) treeNav.scrollBy({ top: -treeNav.clientHeight * 0.8 });
        event.preventDefault();
        break;
      case 'PageDown':
        if (treeNav) treeNav.scrollBy({ top: treeNav.clientHeight * 0.8 });
        event.preventDefault();
        break;
```

- [ ] **Step 6: Verify and commit**

```bash
cargo test assets::tests::browser_js_tree_click_and_aria_contract_is_locked -- --exact
node --check assets/chapbook-browser.js
cargo test
```

Chromium: directory first click expands once, second click collapses, double-click causes no additional action; file first click produces one fragment request/pushState; Tab reaches the tree, PageDown scrolls hidden-scrollbar viewport.

```bash
git add src/assets.rs assets/chapbook-browser.js
git commit -m "feat(tree): open files on first click and add tree semantics"
```

---

### Task 6: Integrated Verification and Completion

**Files:**
- Modify: `docs/superpowers/specs/2026-08-26-tree-reading-layout-design.md`
- Verify: all modified files above

- [ ] **Step 1: Run the full committed test suite**

```bash
cargo fmt --check
cargo clippy --all-targets
cargo test
node --check assets/chapbook-browser.js
```

Expected: fmt clean; clippy zero warnings; all Rust test binaries pass; JS syntax clean.

- [ ] **Step 2: Run the final Chromium matrix**

Use a temporary fixture with a long Markdown/Org document, nested directories, a code file, Unicode names, a literal `+`, literal `%`, literal backslash and a cyclic symlink. Drive real Chromium through the existing raw-CDP approach.

Required evidence:

1. Wide true columns: document/TOC rectangles never overlap at top, middle or bottom.
2. Narrow TOC-first order: TOC rectangle precedes content and DOM order is rail then content.
3. Drag/keyboard resize: clamps at 160/480-or-50vw; reload restores width.
4. Hidden scrollbar: zero scrollbar chrome; wheel, PageDown and touchpad scroll work.
5. Collapse/restore: persistent wide state; drawer state at 500px unaffected.
6. Directory/file clicks: exactly one action; no tree double-click navigation.
7. Host parity: directory and document hosts produce identical tree bounds/gap/layout.
8. Fragment transport matrix: `%41`, mixed-case `%HH`, Unicode, literal `+`, `%`, backslash; malformed escape, `%2F` and dot segments fail closed.
9. SPA lifecycle: exactly one scroll-spy after repeated pane replacement; media teardown unchanged.

- [ ] **Step 3: Run two-axis code review**

Use the repository code-review workflow against the commit before Task 1:

- Standards: AGENTS.md behavior/security contracts + smell baseline.
- Spec: `docs/superpowers/specs/2026-08-26-tree-reading-layout-design.md`.

Fix every blocking/important finding, rerun Step 1 and affected Chromium checks.

- [ ] **Step 4: Record completion and commit**

```bash
git add docs/superpowers/specs/2026-08-26-tree-reading-layout-design.md
git commit -m "docs: record tree reading layout acceptance"
```

Do not push. Report the local commits and verification evidence to the user.
