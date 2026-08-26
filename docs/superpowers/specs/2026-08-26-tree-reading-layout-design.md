# Tree Reading Layout Refinement

**Status:** approved design, awaiting written-spec review  
**Date:** 2026-08-26

## Problem

The current tree-mode UI has four visible problems:

1. The document TOC uses `float + sticky`, so body text can flow below or visually under the TOC instead of occupying a separate column.
2. The tree width is fixed. Its visible vertical scrollbar consumes space even though mouse-wheel, touchpad, touch and keyboard scrolling should remain available.
3. A directory opens on one click, but a file is only selected; opening still requires Enter or double-click.
4. The tree and document pane have no deliberate visual gap.

A fifth requirement was added during design: while reading, the user can manually collapse and restore the tree.

## Chosen Direction

Use visual option A: a true workspace layout rather than card containers.

Wide layout:

```text
resizable tree | 8px separator / gutter | document content | 20px gap | TOC rail
```

The tree, document and TOC are independent regions. Document text never flows into the TOC column.

## Document DOM Seam

Today `#TOC` and every document block are flat children of `.cb-doc`. Applying grid directly would make every heading, paragraph and code block an independent grid item and break document flow.

The progressive enhancement introduces one idempotent seam, `installDocLayout(docEl)`, used for both a full document page and every successfully installed SPA fragment:

```html
<div class="cb-doc">
  <div class="cb-doc-layout">
    <aside class="cb-doc-toc">
      <!-- existing #TOC node is moved here -->
    </aside>
    <div class="cb-doc-content">
      <!-- every remaining original document node is moved here -->
    </div>
  </div>
</div>
```

The TOC remains first in DOM order, matching the current server output and no-JS reading order. It is moved, never cloned or regenerated, so heading IDs, TOC hrefs and the existing slug contract remain unchanged.

Wide CSS uses named areas:

```css
.cb-doc-layout {
  display: grid;
  grid-template-areas: "content toc";
  grid-template-columns: minmax(0, 1fr) 240px;
  gap: 20px;
}
.cb-doc-content { grid-area: content; }
.cb-doc-toc { grid-area: toc; }
```

On a narrow viewport, the named areas become a single column in DOM order:

```css
grid-template-areas:
  "toc"
  "content";
```

No CSS `order` is used. Assistive-technology reading order and keyboard traversal remain TOC then content.

Documents without a TOC stay single-column and do not create an empty rail.

## TOC Layout and Lifecycle

- Wide breakpoint: `75rem`.
- Rail width: about `240px`.
- Rail is sticky inside its own document or pane scroll container, not fixed to the viewport and not floated over body content.
- Long TOCs scroll internally.
- Narrow view keeps the existing top-card presentation.
- `installDocLayout` installs the appropriate scroll-spy after moving the nodes.
- Before an SPA replacement, existing spy disposers run; after replacement the new document layout and observer are installed.
- No-JS keeps the current server DOM and top-card TOC; content remains readable.

## Resizable Tree

### Geometry

- Default width: `238px`.
- Minimum: `160px`.
- Maximum: `min(480px, 50vw)`.
- CSS variable: `--cb-tree-width`.
- The resizer/gutter is about `8px` wide with an approximately `2px` center line.
- The separator provides the requested small visual distance between tree and document pane.

### Pointer and Keyboard Input

The resizer uses Pointer Events and pointer capture so dragging continues when the pointer leaves the handle.

It is keyboard accessible:

- `role="separator"`
- `aria-orientation="vertical"`
- `tabindex="0"`
- `ArrowLeft` / `ArrowRight`: adjust `16px`
- `Home`: minimum width
- `End`: maximum width
- `aria-valuemin`, `aria-valuemax` and `aria-valuenow` update with the width

The width is stored in localStorage as `cb-tree-width`, scoped naturally by browser origin. Reloads, file changes and SPA navigation preserve it. Below `64rem`, the existing drawer ignores the saved width; returning wide restores it.

## Hidden Scrollbar, Preserved Scrolling

The tree keeps vertical scrolling:

```css
overflow-y: auto;
scrollbar-width: none;
```

WebKit additionally hides `::-webkit-scrollbar`. Mouse-wheel, touchpad, touch and keyboard scrolling remain available. Only the scrollbar chrome disappears.

The wide tree viewport is keyboard-focusable and exposes tree semantics:

- `<nav role="tree" tabindex="0" aria-label="目录树">`
- rows use `role="treeitem"` and nested lists use `role="group"`
- the current programmatic-only `tabIndex = -1` is replaced
- after Tab focuses the viewport, Arrow-key tree navigation plus PageUp/PageDown scrolling remain discoverable without a visible scrollbar

## Manual Reading Collapse

Tree collapse is explicit, not automatic.

- localStorage key: `cb-tree-collapsed`.
- A small chevron control lives on the wide resizer.
- Collapsing sets the tree column to zero and hides the resizer, while leaving a compact left-edge restore button.
- Restoring uses the previously saved tree width.
- File navigation never auto-collapses the tree.
- Narrow drawer state is separate and does not read or overwrite the wide collapse state.

## Tree Click Contract

- First directory click: select and expand/collapse; no navigation.
- First file click: select and immediately call the existing SPA `paneNavigate` path.
- Later click events with `event.detail > 1` are ignored.
- The tree-row `dblclick` branch is disabled/no-op in tree mode; filtering click events alone is insufficient because the existing `onDocumentDblclick` path also navigates.
- Enter opens the selected file or directory URL.
- ArrowRight expands only; ArrowLeft collapses or selects the parent.
- The existing tree-expanded and tree-selected persistence remains unchanged.

## Fragment Target Binding

The pane must stay bound to the requested filesystem identity. It must not accept an arbitrary valid wrapper path.

A shared pure function produces a canonical transport comparison key, segment by segment:

1. Split only on literal `/`; never decode separators.
2. Reject malformed `%HH`, encoded slash (`%2F`) and dot-segment forms after normalization. Encoded backslash (`%5C`) remains valid: on the Unix server `\` is a literal filename byte, not a separator.
3. Normalize valid `%HH` hex to uppercase.
4. Decode percent-encoded RFC 3986 unreserved ASCII into literal characters.
5. Keep reserved bytes percent-encoded and uppercase.
6. Encode literal Unicode as uppercase UTF-8 percent bytes.
7. Encode the exact server ASCII path set, including space, `%`, `+`, `#`, `?`, `&`, `=`, quotes, brackets and backslash.
8. Compare the requested key exactly with the wrapper canonical `data-native-path-encoded` key.
9. Only after equality may the pane install DOM or update history; the wrapper value remains the committed canonical identity.

This covers noncanonical `%41`, mixed-case escapes, literal Unicode, literal `+`/`%`/backslash filenames and encoded-slash rejection without weakening the action-path no-decode contract.

## Error Handling

- An invalid saved width falls back to `238px`.
- localStorage failures are ignored without breaking browsing.
- Pointer cancellation ends the resize cleanly and keeps the latest clamped width.
- A fragment identity mismatch remains fail-closed: no DOM installation and no history update.
- SPA replacement preserves the existing abort/generation guards and media/observer teardown.

## Verification

### Committed automation

- `cargo test`
- `cargo clippy --all-targets`
- `cargo fmt --check`
- `node --check assets/chapbook-browser.js`
- Existing Rust codec tests continue to lock server path encoding.

### Chromium acceptance

- Drag to minimum/maximum, release outside the handle, reload, and verify persistence.
- Resize by separator keyboard controls and verify ARIA values.
- Hidden tree scrollbar with wheel, touchpad and keyboard scrolling still functional after Tab focuses the `role="tree"` viewport.
- Directory first click expands exactly once; file first click opens exactly once; double-click creates no second action.
- Wide TOC is a true sibling column; long text never enters beneath it.
- Narrow DOM and visual order are TOC then content.
- Manual collapse/restore persists without affecting drawer state.
- Directory and document hosts produce identical workspace geometry.
- Full-page and SPA replacements install exactly one correct-root scroll-spy.
- Transport-key matrix: mixed-case escapes, Unicode, `+`, `%`, literal-backslash filename, malformed escape, encoded slash and dot segments.

## Out of Scope

- Automatic tree collapse when opening a file.
- New colors, typography or icon redesign.
- Changes to tree sorting, search semantics, native-open security gates or server-side document rendering.
- Replacing the existing narrow drawer design.
