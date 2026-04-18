# Arx: Complete Design & Implementation Specification

**Version:** 0.1.0-draft
**Language:** Rust (2024 edition)
**Website:** arx.dev (pending)

---

## Table of Contents

1. [Design Philosophy](#1-design-philosophy)
2. [Core Architecture](#2-core-architecture)
3. [Buffer Internals: Persistent Rope & Text Properties](#3-buffer-internals)
4. [Rendering Pipeline](#4-rendering-pipeline)
5. [Extension SDK](#5-extension-sdk)
6. [Agent System](#6-agent-system)
7. [Session Daemon & Multi-Client Architecture](#7-session-daemon)
8. [OT/History System](#8-ot-history)
9. [Completion Framework](#9-completion-framework)
10. [LSP & Tree-sitter Integration](#10-lsp-tree-sitter)
11. [Built-in Subsystems](#11-built-in-subsystems)
12. [Configuration System](#12-configuration-system)
13. [Package Manager & Registry](#13-package-manager)
14. [Project Management](#14-project-management)
15. [Keybinding & Input System](#15-keybinding-input)
16. [Performance Targets](#16-performance-targets)
17. [Platform & Build](#17-platform-build)
18. [Development Phases](#18-development-phases)
19. [Open Questions](#19-open-questions)

---

## 1. Design Philosophy

- **Lean core, rich primitives.** The binary ships with powerful built-in data structures and async infrastructure, but almost all user-facing behavior is composed from extensions (written in Rust, compiled as dynamic libraries).
- **Agents are collaborators, not plugins.** Agent interaction is a core abstraction, not bolted on. Buffers, commands, and the event loop are all agent-aware.
- **No scripting language tax.** Extensions are Rust dylibs (`.so`/`.dylib`/`.dll`) loaded at runtime. In the age of AI-assisted development, writing Rust extensions is no harder than writing Elisp — and you get full type safety, async/await, and native performance.
- **Cross-platform first.** Linux, macOS, Windows. Terminal UI is first-class (requiring a modern terminal with Kitty graphics protocol support), with an optional GPU-accelerated graphical frontend.
- **Daemon-native.** The editor runs as a persistent daemon process. Clients are thin rendering endpoints that attach/detach freely — combining the best of Emacs server, tmux, and collaborative editing.

### 1.1 Why No Scripting Language

The traditional argument for Elisp/Lua/Python is low friction for quick scripts. Our counterarguments:

1. **AI writes the boilerplate.** An agent can scaffold a full extension from a natural language description in seconds.
2. **Type safety catches bugs at compile time**, not at 2 AM when your config breaks.
3. **Async/await is native.** No callback hell, no GIL, no coroutine adapters.
4. **One language for core + extensions** = one debugger, one profiler, one ecosystem.
5. **Performance is never a question.** Extensions run at native speed.

For quick one-off automation, the editor provides a **command palette REPL** that can compose and execute registered commands interactively, and an **agent-assist mode** that can generate, compile, and hot-load a one-off extension from a prompt.

---

## 2. Core Architecture

### 2.1 Event Loop & Concurrency

- **Tokio-based async runtime** at the core. Every subsystem (file I/O, LSP, agent communication, rendering, process management) runs as async tasks.
- **Command dispatch** is an async channel-based message bus. Commands are enqueued from any context (keybinding, agent, timer, process output) and dispatched in a deterministic order on the main logic thread.
- **Buffer mutations** are serialized through the main thread (single-writer), but reads are lock-free via a **persistent/immutable rope** (copy-on-write snapshots). Agents and background tasks read consistent snapshots without blocking the editor.

```
┌─────────────────────────────────────────────────┐
│                   Event Loop                     │
│  ┌───────────┐  ┌──────────┐  ┌──────────────┐  │
│  │ Input/Keys│  │  Agents  │  │  Processes   │  │
│  └─────┬─────┘  └────┬─────┘  └──────┬───────┘  │
│        └──────────────┼───────────────┘          │
│                 ┌─────▼──────┐                   │
│                 │ Command Bus│                   │
│                 └─────┬──────┘                   │
│          ┌────────────┼────────────┐             │
│    ┌─────▼─────┐ ┌────▼────┐ ┌────▼─────┐       │
│    │  Buffers  │ │ Windows │ │ Renderer │       │
│    │  (Rope)   │ │ (Splits)│ │ (TUI/GUI)│       │
│    └───────────┘ └─────────┘ └──────────┘       │
└─────────────────────────────────────────────────┘
```

### 2.2 Window & Layout Model

- **Tiling window manager** modeled as a tree of splits (horizontal/vertical), similar to Emacs but with:
  - Named layouts that can be saved/restored
  - Float/overlay windows for popups, completions, agent chat panels
  - Tab groups (like Emacs tab-bar-mode, grouping window configurations)
- **Each window is a view** into a buffer with its own cursor, scroll position, and display parameters.
- **Pixel-level layout** when running in GUI mode; cell-level in terminal mode. The layout engine abstracts over both.

---

## 3. Buffer Internals: Persistent Rope & Text Properties

### 3.1 Why Rope Over Alternatives

**Gap Buffer** (Emacs): Extremely simple and cache-friendly, but concurrent reads are the dealbreaker. Giving agents a consistent snapshot requires either copying the entire buffer (O(n)) or blocking writes during reads. With 10+ agents potentially reading buffers continuously, this is a non-starter.

**Piece Table** (VS Code): Read performance degrades as piece count grows. Snapshotting requires cloning the piece table. Once you augment it with a balanced tree for fast line indexing, you've essentially built a rope with extra indirection.

**Persistent Rope** (Arx's choice): O(log n) everything. Persistent/immutable variants with structural sharing give O(1) snapshots — clone the root Arc, share all unchanged subtrees. Line indexing is built into internal node summaries. Natural fit for OT/CRDT operations. The concurrency model decides this.

### 3.2 Rope Structure

A B-tree (fanout 8–16) where:
- **Leaf nodes** hold text chunks (target ~256 bytes, max 512 bytes) plus local metadata.
- **Internal nodes** hold child pointers plus **cached summaries** aggregated from children.
- **The tree is persistent (copy-on-write).** Edits produce a new root with O(log n) new nodes; unchanged subtrees are shared via `Arc`.

```
             ┌─────────────────────────────┐
             │ Root (v3)                    │
             │ bytes:1847  lines:42  ...    │
             └──────┬──────────────┬────────┘
                    │              │
          ┌─────────▼───┐   ┌─────▼──────────┐
          │ Internal (v3)│   │ Internal (v1)   │  ← shared from v1
          │ bytes:923    │   │ bytes:924       │
          └──┬───┬───┬──┘   └──┬───┬───┬──┬──┘
             │   │   │         │   │   │  │
            v3  v1  v1        v1  v1  v1  v1   ← leaves, mostly shared
```

### 3.3 Summary Trait

Following Zed's approach, the tree is generic over a `Summary` type:

```rust
pub trait Summary: Clone + Default {
    type Item;
    fn summarize(item: &Self::Item) -> Self;
    fn combine(&mut self, other: &Self);
}

#[derive(Clone, Default)]
pub struct TextSummary {
    pub bytes: usize,
    pub chars: usize,
    pub lines: usize,
    pub line_lengths: MaxLineLength,
    pub newline_offsets: CompactOffsets,
}
```

This lets you find "byte offset of line N" in O(log n) by descending the tree and summing `lines` counts — no separate line offset table needed.

### 3.4 Versioned Snapshots

```rust
pub struct Buffer {
    current: Arc<RopeNode>,
    version: u64,
    snapshot_tx: watch::Sender<BufferSnapshot>,
}

pub struct BufferSnapshot {
    pub rope: Arc<RopeNode>,
    pub version: u64,
    pub properties: Arc<PropertyMap>,
}

impl Buffer {
    pub fn snapshot(&self) -> BufferSnapshot { /* O(1) Arc clone */ }

    pub fn edit(&mut self, range: ByteRange, text: &str, origin: EditOrigin) -> EditResult {
        // 1. Apply edit to rope (O(log n), COW)
        // 2. Update properties (shift/split)
        // 3. Record operation in history
        // 4. Bump version, publish snapshot
    }
}
```

### 3.5 Rope Performance

| Operation | Complexity | Notes |
|-----------|-----------|-------|
| Insert/delete at position | O(log n) | COW path from root to leaf |
| Read byte/char at offset | O(log n) | Tree descent |
| Line number → byte offset | O(log n) | Summary aggregation |
| Create snapshot | O(1) | Arc clone |
| Iterate chars in range | O(log n + k) | k = chars in range |
| Apply N-location diff | O(N log n) | Each edit is independent |

### 3.6 Text Properties Architecture

Text properties attach typed metadata to ranges of text. They live in a **separate persistent interval tree** that shares the buffer's versioning scheme. This separation means:

- Properties can be updated without touching the rope
- Multiple property layers can coexist without interfering
- Property-heavy operations don't fragment the text rope
- Different layers can update at different cadences

### 3.7 Property Layers

```rust
pub struct PropertyMap {
    layers: HashMap<LayerId, PropertyLayer>,
}

pub struct PropertyLayer {
    tree: Arc<IntervalTree<PropertyValue>>,
    synced_to: u64,
    adjustment: AdjustmentPolicy,
}

pub enum AdjustmentPolicy {
    TrackEdits,        // Shift ranges to track buffer edits (most common)
    InvalidateOnEdit,  // Invalidate and recompute (e.g., syntax highlighting)
    Static,            // Never adjust (e.g., snapshot annotations)
}
```

Predefined layers:

| Layer | Adjustment | Producer | Purpose |
|-------|-----------|----------|---------|
| `syntax` | InvalidateOnEdit | Tree-sitter | Syntax highlighting scopes |
| `semantic-tokens` | TrackEdits | LSP | Semantic token decorations |
| `diagnostics` | TrackEdits | LSP | Errors, warnings, hints |
| `git-diff` | TrackEdits | forge-vcs | Added/modified/deleted line markers |
| `git-blame` | TrackEdits | forge-vcs | Per-line blame info |
| `fold` | TrackEdits | User/extension | Folded/hidden regions |
| `readonly` | TrackEdits | Extension | Read-only spans |
| `agent-edit` | TrackEdits | Agent system | Attribution for agent-produced text |
| `link` | TrackEdits | Extension | Clickable links with targets |
| `search-match` | Static | Search | Highlighted search results |
| `selection` | Static | Editor core | Selection ranges (multi-cursor) |

### 3.8 Interval Tree Design

Each layer is a **persistent augmented interval tree** — a balanced BST where:
- Each node holds an interval `[start, end)` in byte offsets and a `PropertyValue`.
- Internal nodes cache the maximum endpoint in their subtree.
- Persistence via structural sharing (same COW scheme as the rope).

```rust
pub struct IntervalNode {
    interval: ByteRange,
    value: PropertyValue,
    max_end: usize,
    left: Option<Arc<IntervalNode>>,
    right: Option<Arc<IntervalNode>>,
}

pub enum PropertyValue {
    Scope(ScopeId),
    Diagnostic { severity: Severity, message: Arc<str>, code: Option<Arc<str>> },
    Decoration { face: Face },
    Flag,
    AgentAttribution { agent_id: AgentId, edit_id: u64, timestamp: Instant },
    Extension(Box<dyn Any + Send + Sync>),
}

#[derive(Clone)]
pub struct Face {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<UnderlineStyle>,
    pub strikethrough: Option<bool>,
    pub priority: i16,
}
```

### 3.9 Edit Tracking

When the buffer is edited, `TrackEdits` layers adjust their intervals:

```rust
impl PropertyLayer {
    fn apply_edit(&mut self, edit: &Edit) {
        match self.adjustment {
            AdjustmentPolicy::TrackEdits => {
                // For each interval overlapping or after the edit:
                // - Entirely before: no change
                // - Contains edit point: extend or shrink
                // - Entirely after: shift by delta
                // O(log n + k) where k = affected intervals, COW.
                self.tree = self.tree.adjust(edit.offset, edit.old_len, edit.new_len);
            }
            AdjustmentPolicy::InvalidateOnEdit => {
                self.mark_dirty(edit.affected_range());
            }
            AdjustmentPolicy::Static => {}
        }
    }
}
```

**Sticky behavior** controls what happens when an edit occurs inside a property range:

```rust
pub enum StickyBehavior {
    Grow,       // Insertions at boundaries extend the property
    RearSticky, // Insertions at start don't extend; at end do
    Shrink,     // Insertions at boundaries don't extend
    Split,      // Insertion inside splits the property
}
```

### 3.10 Rendering Query

```rust
impl PropertyMap {
    pub fn styled_runs(&self, range: ByteRange) -> impl Iterator<Item = StyledRun> {
        // 1. Query each visible layer's interval tree for overlapping properties
        // 2. Merge faces by priority (higher priority wins per attribute)
        // 3. Collapse into contiguous runs of identical style
    }
}

pub struct StyledRun {
    pub range: ByteRange,
    pub face: Face,
    pub flags: PropertyFlags,
    pub diagnostics: SmallVec<[Arc<Diagnostic>; 1]>,
}
```

---

## 4. Rendering Pipeline

### 4.1 Architecture Overview

```
  Editor Core (main thread)
       │
       │  publishes ViewState (snapshot + layout + cursors + mode)
       ▼
  ┌─────────────┐
  │  View Layer  │  Pure function: ViewState → RenderTree
  └──────┬──────┘
         │  RenderTree (frame N)
         ▼
  ┌─────────────┐      ┌─────────────────┐
  │  Differ      │◄────│ RenderTree (N-1) │
  └──────┬──────┘      └─────────────────┘
         │  DiffOps (only what changed)
         ▼
  ┌──────┴───────────────────────┐
  │                              │
  ┌▼──────────┐          ┌───────▼──────┐
  │ TUI Backend│          │ GPU Backend  │
  │ (crossterm)│          │ (wgpu)       │
  └────────────┘          └──────────────┘
```

### 4.2 ViewState

```rust
pub struct ViewState {
    pub layout: LayoutTree,
    pub windows: Vec<WindowState>,
    pub overlays: Vec<OverlayState>,
    pub modelines: Vec<ModelineState>,
    pub global: GlobalState,
}

pub struct WindowState {
    pub id: WindowId,
    pub buffer: BufferSnapshot,
    pub cursors: SmallVec<[Cursor; 1]>,
    pub scroll: ScrollPosition,
    pub visible_line_range: Range<usize>,
    pub gutter_config: GutterConfig,
    pub embedded_content: Vec<InlineEmbed>,
}
```

### 4.3 RenderTree

```rust
pub struct RenderTree {
    pub cells: CellGrid,
    pub inline_content: Vec<InlineContent>,
    pub cursor_positions: Vec<CursorRender>,
    pub floating_panels: Vec<FloatingPanel>,
    pub frame_id: u64,
}

#[derive(Clone, PartialEq)]
pub struct Cell {
    pub grapheme: CompactString,
    pub face: ResolvedFace,
    pub flags: CellFlags,
}

bitflags! {
    pub struct CellFlags: u8 {
        const WIDE_CONTINUATION = 0x01;
        const WRAP_POINT        = 0x02;
        const CURSOR_PRIMARY    = 0x04;
        const CURSOR_SECONDARY  = 0x08;
        const DIAGNOSTIC_HINT   = 0x10;
        const SEARCH_MATCH      = 0x20;
    }
}

#[derive(Clone, PartialEq)]
pub struct ResolvedFace {
    pub fg: Rgb,
    pub bg: Rgb,
    pub attrs: FontAttrs,
}

pub enum InlineContent {
    Image { position: CellPosition, size: CellSize, data: ImageData, scale: ScaleMode },
    TerminalEmbed { position: CellPosition, size: CellSize, terminal_id: TerminalId },
    Widget { position: CellPosition, size: CellSize, widget_id: WidgetId, render_fn: Arc<dyn Fn(&mut WidgetCanvas)> },
}
```

### 4.4 Line Rendering Pipeline

For each visible line:

```
Buffer line (rope slice)
    → Text Extraction (rope slice → &str)
    → Property Query (styled_runs from PropertyMap)
    → Theme Resolution (syntax scopes → theme faces → ResolvedFace)
    → Grapheme Segmentation (Unicode → grapheme clusters, wide chars, emoji)
    → Layout (assign graphemes to cell columns, tab expand, soft wrap)
    → Gutter (prepend line number, git status, diagnostics, fold indicators)
    → Row of Cell structs → CellGrid
```

### 4.5 Differ

```rust
pub enum DiffOp {
    CellSpan { row: u16, col_start: u16, cells: Vec<Cell> },
    ScrollRegion { top: u16, bottom: u16, delta: i16 },
    InlineUpdate(InlineContentOp),
    FloatingUpdate(FloatingPanelOp),
    CursorMove(Vec<CursorRender>),
}
```

Diffing strategy:
1. **Scroll detection:** Compare scroll offsets. If shifted by N lines, emit `ScrollRegion` op (terminals handle natively), then diff only newly revealed lines.
2. **Row-level comparison:** Compare cell slices per row. Skip identical rows. Find minimal changed span within changed rows.
3. **Floating panels:** Diffed independently.

Common case costs:
- Typing a character: 1 `CellSpan` (few cells), 1 `CursorMove`.
- Scrolling: 1 `ScrollRegion`, N `CellSpan` for new lines.
- No change: 0 ops.

### 4.6 TUI Backend

```rust
pub struct TuiBackend {
    output: BufWriter<Stdout>,
    size: TermSize,
    shadow: CellGrid,
    image_placements: HashMap<ImageId, KittyPlacement>,
    terminal_embeds: HashMap<TerminalId, TerminalRenderer>,
}
```

Key techniques:
- **Synchronized output** (DEC mode 2026): buffer all output and present atomically — eliminates flicker. Required: Ghostty, Kitty, WezTerm all support it.
- **24-bit color** (SGR 38;2 / 48;2) with delta encoding — only emit attributes that changed.
- **Kitty graphics protocol** for inline images: transmit image data, place with Unicode placeholders.
- **Scroll regions** (DECSTBM + index/reverse-index) instead of full redraw on scroll.

### 4.7 Embedded Terminal Rendering

```rust
pub struct EmbeddedTerminal {
    parser: termwiz::terminal::Terminal,
    grid: CellGrid,
    dirty_rows: BitVec,
    agent_bridge: Option<AgentBridge>,
}

impl EmbeddedTerminal {
    pub fn process_output(&mut self, data: &[u8]) {
        self.parser.advance(data);
    }

    pub fn render_into(&self, target: &mut CellGrid, region: Rect) {
        for row in self.dirty_rows.iter_ones() {
            for col in 0..region.width {
                target.set(region.top + row, region.left + col,
                    self.grid.get(row, col).into_editor_cell());
            }
        }
    }
}
```

### 4.8 GPU Backend

Uses `wgpu` for cross-platform GPU rendering with `cosmic-text` for text shaping.

```
RenderTree → Text Shaper (cosmic-text) → Glyph Atlas Manager → Scene Builder → wgpu Renderer
```

```rust
pub struct GlyphAtlas {
    pages: Vec<wgpu::Texture>,
    cache: HashMap<GlyphKey, AtlasEntry>,
    packer: ShelfPacker,
}

#[repr(C)]
pub struct GlyphInstance {
    pub position: [f32; 2],
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub color: [u8; 4],
}
```

Render passes: (1) background rects, (2) text glyphs (single instanced draw call), (3) line decorations, (4) cursors, (5) inline images. Target: < 2ms per frame on integrated GPUs.

### 4.9 Renderer Trait

```rust
pub trait Renderer: Send + 'static {
    fn init(&mut self) -> Result<TermSize>;
    fn render(&mut self, ops: &[DiffOp]) -> Result<()>;
    fn resize(&mut self, new_size: TermSize) -> Result<()>;
    fn events(&self) -> &dyn Stream<Item = InputEvent>;
    fn capabilities(&self) -> RendererCapabilities;
}

pub struct RendererCapabilities {
    pub inline_images: bool,
    pub true_color: bool,
    pub kitty_keyboard: bool,
    pub unicode_version: UnicodeVersion,
    pub cell_size_px: Option<(u16, u16)>,
    pub synchronized_output: bool,
}
```

### 4.10 Frame Timing

- **TUI:** Event-driven — render only on change. Zero CPU when idle.
- **GPU:** Vsync-driven — run every frame but skip draw call submission if differ produces zero ops.

---

## 5. Extension SDK (`arx-sdk`)

### 5.1 Extension Lifecycle

```rust
#[arx_extension]
pub struct MyExtension {
    state: Mutex<MyState>,
}

#[async_trait]
impl Extension for MyExtension {
    async fn activate(&self, ctx: &mut ActivationContext) -> Result<()>;
    async fn deactivate(&self) -> Result<()> { Ok(()) }
    fn metadata(&self) -> ExtensionMeta;
}

pub struct ExtensionMeta {
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
    pub sdk_version: SemVer,
    pub activation: ActivationPolicy,
    pub commands: &'static [CommandMeta],
    pub dependencies: &'static [Dependency],
}

pub enum ActivationPolicy {
    Startup,
    Language(&'static [&'static str]),
    OnCommand(&'static [&'static str]),
    Project { markers: &'static [&'static str] },
    Manual,
}
```

### 5.2 Context Hierarchy

```rust
/// Full context during activation — maximum access
pub struct ActivationContext {
    commands: CommandRegistry,
    keymaps: KeymapRegistry,
    hooks: HookRegistry,
    buffers: BufferManager,
    windows: WindowManager,
    agents: AgentManager,
    processes: ProcessManager,
    completion: CompletionRegistry,
    fs: FileSystem,
    state: StateStore,
    ui: UiManager,
    diagnostics: DiagnosticManager,
    notifications: NotificationManager,
}

/// Narrowed context for command handlers
pub struct CommandContext {
    pub buffer: Option<BufferHandle>,
    pub window: Option<WindowHandle>,
    pub args: CommandArgs,
    pub editor: EditorHandle,
}

/// Narrowed context for buffer hooks
pub struct BufferContext {
    pub buffer: BufferHandle,
    pub editor: EditorHandle,
}
```

### 5.3 Commands API

```rust
impl CommandRegistry {
    pub fn register<F, Fut>(&mut self, name: &str, handler: F)
    where
        F: Fn(CommandContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send;

    pub fn register_full(&mut self, spec: CommandSpec, handler: impl CommandHandler);
}

pub struct CommandSpec {
    pub name: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub args: &'static [ArgSpec],
    pub hidden: bool,
}
```

### 5.4 Keymaps API

```rust
impl KeymapRegistry {
    pub fn bind(&mut self, scope: &str, keys: &str, command: &str);
    pub fn bind_when(&mut self, scope: &str, keys: &str, command: &str, when: WhenClause);
    pub fn create_scope(&mut self, name: &str, parent: &str);
}

pub enum WhenClause {
    Language(String),
    StateIs(String),
    And(Vec<WhenClause>),
    Or(Vec<WhenClause>),
    Not(Box<WhenClause>),
}
```

### 5.5 Buffers API

```rust
#[derive(Clone)]
pub struct BufferHandle { /* ... */ }

impl BufferHandle {
    pub fn snapshot(&self) -> BufferSnapshot;       // O(1), lock-free
    pub fn id(&self) -> BufferId;
    pub fn path(&self) -> Option<&Path>;
    pub fn language(&self) -> Option<&str>;
    pub fn is_modified(&self) -> bool;
    pub fn version(&self) -> u64;

    pub async fn edit(&self, range: impl Into<EditRange>, replacement: &str, origin: EditOrigin) -> Result<EditResult>;
    pub async fn edit_batch(&self, edits: Vec<(EditRange, String)>, origin: EditOrigin) -> Result<EditResult>;

    pub fn state<T: Send + Sync + 'static>(&self) -> Option<Arc<T>>;
    pub async fn set_state<T: Send + Sync + 'static>(&self, value: T);

    pub fn properties(&self) -> &PropertyMap;
    pub async fn set_property(&self, layer: &str, range: ByteRange, value: PropertyValue, sticky: StickyBehavior);
    pub async fn clear_property_layer(&self, layer: &str);

    pub fn on_change(&self) -> impl Stream<Item = BufferChangeEvent>;
}

pub struct BufferSnapshot { /* immutable rope + property map */ }

impl BufferSnapshot {
    pub fn text(&self) -> Cow<str>;
    pub fn slice(&self, range: ByteRange) -> RopeSlice;
    pub fn line(&self, n: usize) -> Option<RopeSlice>;
    pub fn line_count(&self) -> usize;
    pub fn byte_to_line(&self, byte: usize) -> usize;
    pub fn line_to_byte(&self, line: usize) -> usize;
    pub fn byte_to_point(&self, byte: usize) -> Point;
    pub fn point_to_byte(&self, point: Point) -> usize;
    pub fn find(&self, pattern: &str) -> Vec<ByteRange>;
    pub fn find_regex(&self, pattern: &Regex) -> Vec<ByteRange>;
    pub fn properties(&self) -> &PropertyMap;
    pub fn styled_runs(&self, range: ByteRange) -> impl Iterator<Item = StyledRun>;
}

pub enum EditOrigin { User, Extension(&'static str), Agent(AgentId), Undo, Lsp }
pub enum EditRange { Byte(Range<usize>), Point(Range<Point>), Line(Range<usize>) }
```

### 5.6 Buffer Manager API

```rust
impl BufferManager {
    pub async fn open(&self, path: impl AsRef<Path>) -> Result<BufferHandle>;
    pub fn scratch(&self, name: &str, language: Option<&str>) -> BufferHandle;
    pub fn find_by_path(&self, path: &Path) -> Option<BufferHandle>;
    pub fn get(&self, id: BufferId) -> Option<BufferHandle>;
    pub fn all(&self) -> impl Iterator<Item = BufferHandle>;
    pub fn on_open(&self) -> impl Stream<Item = BufferHandle>;
    pub fn on_close(&self) -> impl Stream<Item = BufferId>;
}
```

### 5.7 Windows & Layout API

```rust
impl WindowManager {
    pub fn focused(&self) -> Option<WindowHandle>;
    pub async fn split(&self, window: WindowHandle, direction: SplitDirection, buffer: BufferHandle) -> Result<WindowHandle>;
    pub async fn float(&self, config: FloatConfig) -> Result<WindowHandle>;
    pub async fn close(&self, window: WindowHandle) -> Result<()>;
    pub async fn focus(&self, window: WindowHandle);
    pub async fn focus_direction(&self, dir: Direction);
    pub async fn save_layout(&self, name: &str);
    pub async fn restore_layout(&self, name: &str) -> Result<()>;
    pub fn all(&self) -> impl Iterator<Item = WindowHandle>;
}

impl WindowHandle {
    pub fn id(&self) -> WindowId;
    pub fn buffer(&self) -> BufferHandle;
    pub fn cursors(&self) -> Vec<Cursor>;
    pub fn visible_range(&self) -> Range<usize>;
    pub fn size(&self) -> CellSize;
    pub async fn set_buffer(&self, buf: BufferHandle);
    pub async fn set_cursor(&self, pos: Point);
    pub async fn scroll_to(&self, line: usize);
}

pub struct FloatConfig {
    pub buffer: BufferHandle,
    pub anchor: FloatAnchor,
    pub size: FloatSize,
    pub border: BorderStyle,
    pub focusable: bool,
    pub close_on_focus_loss: bool,
}
```

### 5.8 Hooks API

```rust
impl HookRegistry {
    pub fn on_buffer_change<F, Fut>(&mut self, f: F)
    where F: Fn(BufferContext, BufferChangeEvent) -> Fut + Send + Sync + 'static,
          Fut: Future<Output = Result<()>> + Send;

    pub fn on_buffer_open<F, Fut>(&mut self, f: F);
    pub fn on_before_save<F, Fut>(&mut self, f: F);
    pub fn on_after_save<F, Fut>(&mut self, f: F);
    pub fn on_focus_change<F, Fut>(&mut self, f: F);
    pub fn on_command<F, Fut>(&mut self, f: F);
    pub fn on_cursor_move<F, Fut>(&mut self, f: F);

    pub fn on_idle<F, Fut>(&mut self, delay: Duration, f: F);
    pub fn on_custom<T: CustomEvent, F, Fut>(&mut self, f: F);
    pub fn emit_custom<T: CustomEvent>(&self, event: T);
}
```

### 5.9 Completion API

```rust
impl CompletionRegistry {
    pub fn register_source(&mut self, source: impl CompletionSource);
}

#[async_trait]
pub trait CompletionSource: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn priority(&self) -> i32 { 0 }
    fn applicable(&self, buffer: &BufferSnapshot) -> bool { true }
    async fn complete(&self, ctx: CompletionContext) -> Result<Option<Vec<CompletionItem>>>;
    async fn resolve(&self, item: &mut CompletionItem) -> Result<()> { Ok(()) }
}

pub struct CompletionItem {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
    pub documentation: Option<Markup>,
    pub edit: CompletionEdit,
    pub source: String,
    pub sort_key: Option<String>,
    pub filter_text: Option<String>,
}

pub enum CompletionEdit {
    Plain(String),
    Snippet(String),
    TextEdit { range: EditRange, new_text: String },
    Multi(Vec<(EditRange, String)>),
}
```

### 5.10 Processes API

```rust
impl ProcessManager {
    pub async fn spawn(&self, config: ProcessConfig) -> Result<ProcessHandle>;
    pub async fn spawn_terminal(&self, config: TerminalConfig) -> Result<TerminalHandle>;
}

impl ProcessHandle {
    pub fn id(&self) -> ProcessId;
    pub fn stdin(&self) -> &dyn AsyncWrite;
    pub fn stdout(&self) -> impl Stream<Item = Bytes>;
    pub fn stderr(&self) -> impl Stream<Item = Bytes>;
    pub async fn wait(&self) -> Result<ExitStatus>;
    pub async fn kill(&self) -> Result<()>;
}

impl TerminalHandle {
    pub fn id(&self) -> TerminalId;
    pub async fn write(&self, data: &[u8]) -> Result<()>;
    pub fn grid(&self) -> &CellGrid;
    pub fn on_output(&self) -> impl Stream<Item = TerminalEvent>;
    pub async fn attach_agent_bridge(&self, agent: AgentHandle) -> Result<()>;
    pub async fn show_in(&self, window: WindowHandle);
}

pub struct SandboxConfig {
    pub filesystem: FsPolicy,
    pub network: NetworkPolicy,
    pub process: ProcessPolicy,
    pub backend: SandboxBackend,   // Bubblewrap, Landlock, SandboxExec, None
}
```

### 5.11 Agents API

```rust
impl AgentManager {
    pub fn register_adapter(&mut self, adapter: impl AgentAdapter);
    pub async fn spawn(&self, adapter_name: &str, config: AgentConfig) -> Result<AgentHandle>;
    pub fn active(&self) -> impl Iterator<Item = AgentHandle>;
}

impl AgentHandle {
    pub fn id(&self) -> AgentId;
    pub fn adapter_name(&self) -> &str;
    pub async fn send(&self, msg: AgentMessage) -> Result<()>;
    pub fn events(&self) -> impl Stream<Item = AgentEvent>;
    pub async fn request_edit(&self, buffer: &BufferSnapshot, instruction: &str) -> Result<EditProposal>;
    pub async fn ask(&self, prompt: &str) -> Result<String>;
}
```

### 5.12 File System, State, UI, Diagnostics APIs

```rust
impl FileSystem {
    pub async fn read(&self, path: impl AsRef<Path>) -> Result<Vec<u8>>;
    pub async fn read_to_string(&self, path: impl AsRef<Path>) -> Result<String>;
    pub async fn write(&self, path: impl AsRef<Path>, data: &[u8]) -> Result<()>;
    pub async fn read_dir(&self, path: impl AsRef<Path>) -> Result<Vec<DirEntry>>;
    pub async fn find_files(&self, root: impl AsRef<Path>, pattern: &str) -> Result<Vec<PathBuf>>;
    pub fn watch(&self, path: impl AsRef<Path>) -> impl Stream<Item = FsEvent>;
    pub async fn find_project_root(&self, from: impl AsRef<Path>, markers: &[&str]) -> Option<PathBuf>;
}

impl StateStore {
    pub fn set<T: Send + Sync + 'static>(&self, value: T);
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>>;
    pub fn set_flag(&self, key: &str, value: bool);
    pub fn get_flag(&self, key: &str) -> bool;
    pub fn watch<T: Send + Sync + 'static>(&self) -> impl Stream<Item = Arc<T>>;
}

impl UiManager {
    pub async fn input(&self, config: InputConfig) -> Result<Option<String>>;
    pub async fn select<T: ToString>(&self, config: SelectConfig<T>) -> Result<Option<T>>;
    pub async fn multi_select<T: ToString>(&self, config: SelectConfig<T>) -> Result<Vec<T>>;
    pub fn notify(&self, level: NotifyLevel, message: &str);
    pub async fn sidebar(&self, side: Side, config: SidebarConfig) -> Result<SidebarHandle>;
    pub async fn set_virtual_text(&self, buffer: &BufferHandle, line: usize, text: &str, face: Face, position: VirtualTextPosition);
}

impl DiagnosticManager {
    pub async fn publish(&self, buffer: BufferId, source: &str, diagnostics: Vec<Diagnostic>);
    pub fn get(&self, buffer: BufferId) -> Vec<Diagnostic>;
    pub fn on_change(&self) -> impl Stream<Item = (BufferId, Vec<Diagnostic>)>;
}
```

### 5.13 ABI Boundary

The `#[arx_extension]` proc macro generates ABI-stable wrappers via the `abi_stable` crate:

```rust
// Auto-generated
#[no_mangle]
pub extern "C" fn arx_extension_create() -> abi_stable::RBox<dyn SExtension> {
    abi_stable::RBox::new(MyExtension::default())
}

#[no_mangle]
pub extern "C" fn arx_sdk_version() -> abi_stable::std_types::RString {
    env!("ARX_SDK_VERSION").into()
}
```

Extensions are loaded via `libloading` with SDK version compatibility check.

### 5.14 WASM Extension Support (Secondary)

Extensions can also target `wasm32-wasip2` for sandboxed/portable distribution. Same source code compiles to both native dylib and WASM. WASM mode has limitations (no direct filesystem, no process spawning, higher call overhead) but is fully sandboxed and safe from untrusted sources. Both formats can run simultaneously.

### 5.15 Extension Hot-Reload

- Extensions are `.so`/`.dylib` files in `~/.arx/extensions/`.
- The editor can unload/reload extensions without restarting via `libloading` + careful ABI boundary design.
- `arx ext dev` watches source, rebuilds, and hot-loads on change during development.

---

## 6. Agent System

### 6.1 Agent as First-Class Primitive

```rust
#[async_trait]
pub trait AgentAdapter: Send + Sync + 'static {
    fn name(&self) -> &str;
    async fn spawn(&self, config: AgentConfig) -> Result<Box<dyn AgentSession>>;
}

#[async_trait]
pub trait AgentSession: Send + Sync {
    async fn send(&self, msg: AgentMessage) -> Result<()>;
    async fn recv(&self) -> Result<AgentEvent>;
    fn capabilities(&self) -> &AgentCapabilities;
    async fn shutdown(&self) -> Result<()>;
}

pub struct AgentCapabilities {
    pub can_edit_buffers: bool,
    pub can_run_commands: bool,
    pub can_spawn_processes: bool,
    pub can_access_filesystem: bool,
    pub sandboxed: bool,
}
```

### 6.2 Agent Communication Protocol

```rust
/// Editor → Agent
pub enum ToAgent {
    Init { request_id: u64, context: AgentContext },
    Instruction { request_id: u64, text: String, context: AgentContext },
    ToolResult { call_id: String, result: ToolResultPayload },
    EditDecision { proposal_id: u64, decision: EditDecision },
    Cancel { request_id: u64 },
    Ping,
}

pub struct AgentContext {
    pub buffers: Vec<AgentBufferView>,
    pub file_tree: Option<FileTree>,
    pub diagnostics: Vec<Diagnostic>,
    pub cursors: Vec<AgentCursorInfo>,
    pub project: Option<ProjectInfo>,
    pub granted_capabilities: AgentCapabilities,
}

/// Agent → Editor
pub enum FromAgent {
    TextDelta { request_id: u64, text: String },
    EditProposal(EditProposal),
    ToolCall(AgentToolCall),
    Done { request_id: u64 },
    Error { request_id: u64, message: String },
    Pong,
}
```

### 6.3 Agent Tool Calls

```rust
pub enum AgentToolCall {
    ReadFile { call_id: String, path: PathBuf, range: Option<LineRange> },
    WriteFile { call_id: String, path: PathBuf, content: String },
    EditBuffer { call_id: String, path: PathBuf, edits: Vec<ProposedEdit> },
    RunCommand { call_id: String, command: String, args: Vec<String>, cwd: Option<PathBuf>, timeout: Option<Duration> },
    Search { call_id: String, query: String, kind: SearchKind, scope: SearchScope },
    ListFiles { call_id: String, path: PathBuf, pattern: Option<String>, recursive: bool },
    GetDiagnostics { call_id: String, path: Option<PathBuf> },
    AskUser { call_id: String, question: String, options: Option<Vec<String>> },
}
```

### 6.4 Edit Proposals & Permissions

Agents don't write to buffers directly. They submit `EditProposal` objects that the user can accept, reject, or modify — unless running in "autonomous mode" where approved agents auto-apply.

```rust
pub struct EditProposal {
    pub id: u64,
    pub agent_id: AgentId,
    pub buffer_id: BufferId,
    pub base_version: u64,
    pub edits: Vec<ProposedEdit>,
    pub description: Option<String>,
}

pub struct ProposedEdit {
    pub range: EditRange,
    pub new_text: String,
    pub reason: Option<String>,
}
```

Every agent action is recorded in an audit log — which agent, what it did, what it read, when. Viewable in a dedicated `*agent-log*` buffer.

### 6.5 Built-in Agent Adapters

| Adapter | Protocol | Notes |
|---------|----------|-------|
| Claude Code | Stdin/stdout CLI wrapper | Embed sessions in a PTY, parse structured output |
| Anthropic API | HTTP/SSE streaming | Direct API integration with tool use |
| OpenAI-compatible | HTTP/SSE | Any OpenAI-compatible endpoint |
| LSP as Agent | LSP protocol | Treat LSP servers as special-case agents |
| Custom | User-defined via `AgentAdapter` trait | Extension authors add any backend |

### 6.6 TTY Bridge

The TTY bridge connects an agent running in an embedded terminal (like Claude Code in a PTY) to the structured agent protocol.

CLI agents can embed structured data using **OSC escape sequences** (custom code 7741):

```
\x1b]7741;<type>;<json-payload>\x07
```

Types: `edit`, `tool`, `done`. Any CLI tool can participate as an Arx agent by emitting these sequences. A thin wrapper library (`libarx-agent`) makes this trivial for any language.

For agents that don't emit OSC sequences (stock Claude Code, raw shell commands), the bridge falls back to **heuristic parsing** — detecting file-write patterns, diff blocks, and common output formats.

```rust
pub struct TtyBridge {
    terminal: TerminalHandle,
    parser: BridgeParser,
    to_editor: mpsc::Sender<FromAgent>,
    from_editor: mpsc::Receiver<ToAgent>,
    agent_id: AgentId,
}
```

### 6.7 Agent-in-TTY Workflow

A signature feature: an agent running inside an embedded terminal as a first-class editing participant.

- Agent requests "open file X" → editor opens buffer
- Agent requests "edit lines 10-20 of foo.rs" → editor applies diff with attribution
- Agent requests "run tests" → editor spawns process, captures output
- User sees agent's conversational output in terminal pane while edits appear live in editor buffers with diff highlighting
- Multiple agent terminals can run concurrently

---

## 7. Session Daemon & Multi-Client Architecture

### 7.1 Overview

Arx runs as a **daemon process** (`arxd`) that owns all editor state. Clients are thin rendering endpoints that connect, receive a view stream, and send input events. This unifies Emacs server, tmux, and collaborative editing.

```
┌───────────────────────────────────────┐
│           Arx Daemon (arxd)            │
│  Buffers, Agents, Processes, Extensions│
│  ┌────────────────────────────────┐   │
│  │       Session Manager           │   │
│  │  Session:0  Session:1  Session:2│   │
│  └────────────────────────────────┘   │
│  Unix socket / TCP listener            │
└───────┬──────────┬──────────┬─────────┘
        │          │          │
   TUI client  GPU client  Headless
   (terminal)  (desktop)   (CI/scripts)
```

### 7.2 Daemon Lifecycle

- Socket: `$XDG_RUNTIME_DIR/arx/<name>.sock` (default name: `default`)
- Auto-start: `arx` command auto-starts daemon if not running, then connects.
- Persistence: daemon survives client disconnects. Buffers, agents, processes all persist.
- Graceful shutdown serializes state to `~/.local/state/arx/<name>/` for restore on next start.
- Optional idle timeout (default: never).

### 7.3 Sessions

A **session** is a named window layout + per-window state. Analogous to tmux session.

```rust
pub struct Session {
    pub id: SessionId,
    pub name: String,
    pub layout: LayoutTree,
    pub window_states: HashMap<WindowId, WindowViewState>,
    pub attached_clients: HashSet<ClientId>,
    pub working_dir: PathBuf,
    pub project: Option<ProjectId>,
    pub created_at: Instant,
    pub last_accessed: Instant,
}
```

**Multi-client on same session:** layout is shared, cursors are independent per client (other clients' cursors appear as dim secondary cursors), input is merged onto the command bus.

**Multi-client on different sessions:** each client views different layouts/buffers, sharing the same daemon (buffer pool, agents, extensions).

### 7.4 Client-Daemon Protocol

Length-prefixed binary over Unix sockets (or TLS-wrapped TCP for remote):

```rust
/// Client → Daemon
pub enum ClientMessage {
    Hello { protocol_version: u32, client_type: ClientType, terminal_size: Option<TermSize>, capabilities: ClientCapabilities, attach_to: AttachTarget },
    Input(InputEvent),
    Resize(TermSize),
    Command { name: String, args: CommandArgs },
    Detach,
    Ping(u64),
}

pub enum AttachTarget {
    Session(String),
    LastUsed,
    New { name: Option<String>, working_dir: Option<PathBuf> },
    File(PathBuf),
}

/// Daemon → Client
pub enum DaemonMessage {
    Welcome { session_id: SessionId, protocol_version: u32, daemon_capabilities: DaemonCapabilities },
    Render(RenderPayload),
    CursorUpdate(Vec<CursorRender>),
    Notification { level: NotifyLevel, message: String },
    SessionChanged(SessionSnapshot),
    Pong(u64),
}

pub enum RenderPayload {
    CellDiff(Vec<DiffOp>),       // TUI clients
    FullGrid(CellGrid),          // TUI clients (initial/resync)
    Scene(SceneDescription),     // GPU clients
    Structured(StructuredUpdate),// Headless clients
}
```

### 7.5 Bandwidth Optimization

Per-client `last_frame` shadow grid for minimal diff transmission. Rate limiting for slow connections (skip intermediate frames). Optional zstd compression.

### 7.6 TUI Client

Extremely thin: no buffer management, no extensions, no agents. Pure I/O. Starts in < 10ms when connecting to existing daemon. Client crashes don't affect editor state.

### 7.7 Remote Access

- **SSH tunneling** (recommended): `ssh -L /tmp/arx-remote.sock:/run/user/1000/arx/default.sock devbox`
- **Native TCP with TLS + auth**: pre-shared token, mTLS, or Single Packet Authorization
- **TRAMP-style**: `arx open /ssh:devbox:/home/matt/project/main.rs` — local daemon proxies file I/O over SSH

### 7.8 Systemd Integration

```ini
[Unit]
Description=Arx Editor Daemon

[Service]
Type=notify
ExecStart=/usr/bin/arx daemon --systemd
Restart=on-failure

[Install]
WantedBy=default.target
```

Supports `sd_notify`, status reporting, and socket activation.

### 7.9 Agent Synergy

- Agents survive client disconnects — continue working while you're away
- Agent-only headless sessions for background tasks
- EditProposals queued for review on reconnect
- `*agent-log*` buffer shows complete timeline

---

## 8. OT/History System

### 8.1 Operation Model

Every edit is a structured **Operation**:

```rust
pub struct Operation {
    pub id: OperationId,
    pub buffer_id: BufferId,
    pub base_version: u64,
    pub result_version: u64,
    pub timestamp: Instant,
    pub origin: EditOrigin,
    pub components: Vec<OpComponent>,
    pub inverse: Option<Box<Operation>>,
}

pub enum OpComponent {
    Retain(usize),
    Insert(String),
    Delete(usize),
}
```

### 8.2 Operational Transform

When an agent generates an edit against version N but the buffer is now at N+3, the edit is transformed against the three intervening operations:

```rust
pub fn transform(op_a: &Operation, op_b: &Operation) -> Result<(Operation, Operation)> {
    // Standard OT transform for insert/delete operations.
    // Property: apply(apply(doc, op_a), op_b') == apply(apply(doc, op_b), op_a')
    // Handles: Insert vs Insert, Insert vs Retain, Insert vs Delete,
    //          Delete vs Retain, Delete vs Delete, Retain vs Retain
}
```

Transform pipeline: `E' = transform(E, O6)` → `E'' = transform(E', O7)` → `E''' = transform(E'', O8)` → apply E''' to buffer.

### 8.3 Undo Tree

Branching undo tree — no history is ever lost:

```rust
pub struct UndoTree {
    nodes: Vec<UndoNode>,
    current: usize,
    root: usize,
}

pub struct UndoNode {
    pub id: usize,
    pub operation: Operation,
    pub inverse: Operation,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub timestamp: Instant,
    pub origin: EditOrigin,
    pub branch_id: u64,
}

impl UndoTree {
    pub fn undo(&mut self, buffer: &mut Buffer) -> Result<()>;
    pub fn redo(&mut self, buffer: &mut Buffer, branch: Option<u64>) -> Result<()>;
    pub fn visualize(&self) -> UndoTreeView;

    /// Selective undo by origin — undo only agent edits, or only user edits.
    /// Uses OT to transform the inverse operation against intervening operations.
    pub fn undo_by_origin(&mut self, buffer: &mut Buffer, origin: &EditOrigin) -> Result<()>;
}
```

### 8.4 Undo Grouping

```rust
pub struct UndoGrouper {
    pub timeout: Duration,           // default: 500ms
    pub max_group_size: usize,       // default: 100
    pub break_on: UndoBreakPolicy,   // Whitespace, Newline, or Never
}
```

Agent edits are always their own undo group.

---

## 9. Completion Framework

### 9.1 Pipeline

```
Keystroke → Trigger Detection → Source Dispatch (parallel, with timeout)
         → Merge & Dedup → Filter & Score (fuzzy) → Sort & Rank → Render popup
```

```rust
pub struct CompletionEngine {
    sources: Vec<Arc<dyn CompletionSource>>,
    matcher: FuzzyMatcher,
    config: CompletionConfig,
}

pub struct CompletionConfig {
    pub auto_trigger_chars: HashSet<char>,
    pub auto_trigger_min_chars: usize,    // default: 1
    pub source_timeout: Duration,          // default: 200ms
    pub max_results: usize,               // default: 50
    pub preselect: PreselectPolicy,
    pub sorting: SortingConfig,
}
```

Sources are fanned out concurrently. Slow sources are timed out. Results are fuzzy-filtered against the user's typed prefix.

### 9.2 Built-in Sources

- **LspCompletionSource** — highest quality for supported languages
- **BufferWordSource** — words from all open buffers (fast, always available)
- **FilePathSource** — triggered by "/" or path separators
- **SnippetSource** — user-defined and language-specific templates
- **AgentCompletionSource** — AI-powered completions (slower)
- **CommandSource** — command names for palette / M-x
- **RecentSource** — boost previously selected items

### 9.3 Fuzzy Matching

```rust
pub enum MatchAlgorithm {
    FzfV2,      // Characters in order, prefer consecutive + word boundary matches
    Orderless,  // All query words must appear, any order
    Prefix,     // Exact prefix match only
}
```

---

## 10. LSP & Tree-sitter Integration

### 10.1 LSP Client

```rust
pub struct LspManager {
    servers: HashMap<LspKey, LspServer>,
    config: HashMap<String, LspServerConfig>,
}
```

LSP features map to Arx subsystems:

| LSP Feature | Arx Integration |
|-------------|----------------|
| `semanticTokens` | `semantic-tokens` property layer |
| `publishDiagnostics` | `diagnostics` property layer + DiagnosticManager |
| `completion` | LspCompletionSource |
| `hover` | floating overlay window |
| `definition` | `lsp:goto-definition` command |
| `codeAction` | command palette entries, inline lightbulb |
| `formatting` | `lsp:format-buffer` command |
| `inlayHint` | virtual text annotations |
| `rename` | multi-buffer edit proposal |

Document sync: incremental (derived from Operation components) or full sync fallback.

### 10.2 Tree-sitter

Provides fast, incremental syntax parsing for highlighting and structural editing.

```rust
pub struct TreeSitterState {
    parser: tree_sitter::Parser,
    tree: Option<tree_sitter::Tree>,
    highlights_query: tree_sitter::Query,
}
```

After every buffer edit:
1. Tree-sitter incrementally re-parses (typically < 1ms)
2. `syntax` property layer marked dirty for affected region
3. On next render, re-highlight only dirty lines
4. Results cached until next edit

Structural editing commands: `ast:select-parent`, `ast:next-sibling`, `ast:prev-sibling`, `ast:select-inner`, `ast:swap-next`, `ast:swap-prev`.

---

## 11. Built-in Subsystems (Core Extensions)

Ship with the editor but implemented as extensions against the SDK:

### 11.1 Version Control (`arx-vcs`)
- Git-first but trait-based for other VCS backends
- Inline blame, gutter diff indicators, branch/status in modeline
- Magit-inspired interactive rebase/commit/log interface
- Async — all git operations are non-blocking

### 11.2 Org Mode / Markdown (`arx-docs`)
- Org-mode–inspired structured document editing
- Outline folding via text properties
- TODO states, tags, properties
- Embedded code blocks with execution (agent-powered or direct)
- Table editing with formula support
- Full CommonMark + GFM Markdown with live preview

### 11.3 Embedded Terminal
- Terminal emulation via termwiz (from WezTerm) or vte
- Multiple instances in any split or overlay
- Full Kitty keyboard protocol support
- Shell integration (OSC 133 prompt marking, OSC 7 CWD tracking)
- Terminal-to-buffer promotion
- Agent-aware terminals get the bridge layer automatically

---

## 12. Configuration System

### 12.1 Hybrid TOML + Rust

```
~/.config/arx/
├── config.toml          # Declarative settings
├── keys.toml            # Keybinding overrides
├── theme.toml           # Theme overrides
├── init/                # Rust config crate (procedural logic)
│   ├── Cargo.toml
│   └── src/lib.rs
└── extensions.toml      # Extension list + per-extension config
```

### 12.2 config.toml

```toml
[editor]
font = "Iosevka"
font_size = 14
tab_width = 4
insert_spaces = true
line_numbers = "relative"
cursor_blink = false
scroll_margin = 5
word_wrap = "off"
whitespace_render = "trailing"

[theme]
name = "catppuccin-mocha"

[completion]
auto_trigger = true
min_chars = 1
max_results = 50
source_timeout_ms = 200

[terminal]
shell = "/bin/zsh"
scrollback = 10000

[daemon]
auto_start = true
idle_timeout = "never"
socket_dir = "$XDG_RUNTIME_DIR/arx"
```

### 12.3 keys.toml

```toml
[global]
"C-x C-f" = "file:open"
"C-x b"   = "buffer:switch"
"C-x d"   = "session:detach"
"M-x"     = "command-palette"
"C-c a t" = "agent:toggle-panel"

[lang.rust]
"C-c C-c" = "cargo:build"
"C-c C-t" = "cargo:test"
```

### 12.4 extensions.toml

```toml
[extensions]
git-gutter = { version = "0.3", enabled = true }
arx-lsp = { version = "0.2", enabled = true }
magit = { version = "0.1", enabled = true }

[extensions.arx-lsp.config]
rust-analyzer = { path = "rust-analyzer" }
```

### 12.5 Procedural Config (init/src/lib.rs)

```rust
use arx_sdk::prelude::*;

#[arx_config]
pub fn setup(arx: &mut Arx) -> Result<()> {
    arx.commands.register("my:open-notes", |ctx| async move { /* ... */ });

    arx.hooks.on_project_open(|project, ctx| async move {
        if project.has_file("Cargo.toml") {
            ctx.commands.exec("arx-lsp:start", &["rust-analyzer"]).await?;
        }
        Ok(())
    });

    Ok(())
}
```

### 12.6 Config Reload

- TOML changes: watched via inotify/kqueue, applied instantly
- Rust init crate: `arx config build` recompiles; daemon hot-reloads
- Theme/keybinding changes: instant, no restart

### 12.7 v1 Implementation (shipping)

The above §12.1–§12.6 describe the long-term target. What ships
today is a single `config.toml` file (no separate `keys.toml` /
`theme.toml` / init crate yet) loaded by the `arx-config` crate.
Hot-reload is deferred to v2 — relaunch to pick up changes.

**Discovery order** (first hit wins):

1. `--config <path>` CLI flag. Hard error if missing or unparseable.
2. `$ARX_CONFIG` env var. Same hard-fail semantics.
3. Platform default:
   - Linux/macOS: `$XDG_CONFIG_HOME/arx/config.toml`, else
     `$HOME/.config/arx/config.toml`.
   - Windows: `%APPDATA%\arx\config.toml`, else
     `%USERPROFILE%\arx\config.toml`.
4. Missing default-path file → `Config::default()` silently.
5. `--no-config` skips discovery entirely.

**Schema**:

```toml
[keymap]
profile = "emacs"          # "emacs" | "vim" | "kedit"

# Applied in file order, after the profile is built.
[[keymap.bindings]]
keys = "C-c p"
command = "command-palette.open"

# Shadows anything inherited from the profile.
[[keymap.unbind]]
keys = "C-z"

[features]                 # All default to true.
syntax = true
lsp = true
mouse = true
kitty_keyboard_protocol = true
extensions = true

[appearance]
theme = "one-dark"
line_numbers = true
# Tokens: {name}, {modified}, {line}, {total}, {bytes}, {mode}.
# Unknown tokens render literally. `None` = built-in default.
status_format = "{name}{modified}  (ln {line}/{total})"

# User overrides win over built-ins by `language_id`. New ids extend
# the registry when `extensions` is supplied. `initialization_options`
# is arbitrary TOML, converted to JSON at spawn time.
[[lsp.servers]]
language_id = "python"
command = "pylsp"
args = ["--stdio"]
extensions = ["py"]
root_markers = ["pyproject.toml"]

[lsp.servers.initialization_options]
"pylsp.plugins.ruff.enabled" = true
```

**Semantics**:

- `[keymap].profile` — overridden by `--keymap {emacs,vim,kedit}` on
  the CLI when provided.
- `[[keymap.bindings]]` / `[[keymap.unbind]]` — applied to
  `profile.global` via `Arc::make_mut` + the existing
  `Keymap::bind_str` / `Keymap::unbind`. v1 does not target modal
  layers (e.g. `vim.normal`); that requires a `layer = "..."`
  field added later.
- `[features]` — runtime toggles layered on top of the existing
  Cargo feature gates. A binary built `--no-default-features` still
  compiles out the code for `syntax` and `lsp`; a default build
  honours the runtime boolean. Mouse and Kitty flags are consulted
  in `TerminalGuard::enable`; `extensions` is read in the daemon's
  extension-host bootstrap.
- `[appearance].theme` — looked up via
  `arx_highlight::Theme::by_name`. v1 registers `"one-dark"`,
  `"default"`, and `"dark"` as aliases for the built-in theme;
  unknown names emit a warning and fall back to the default.
- `[[lsp.servers]]` — merged into the static registry via
  `arx_lsp::LspRegistry::with_overrides`. Resolution: override hit
  by `language_id` first, then built-in by `language_id`, then
  extension lookup in the same order.

**Programmatic customization** that TOML can't express stays in the
Rust extension SDK (`arx-sdk`). Extensions register commands and
keybindings programmatically at activation; users compose TOML for
the declarative 90% and cdylibs for the procedural 10%.

**Validation & error UX**:

- Hard errors (non-zero exit): TOML parse error in an explicit
  `--config` file, missing explicit config path, invalid
  `profile = "..."` value.
- Warnings (collected into a `Vec<Warning>`): unknown command name,
  invalid key-sequence syntax, unknown theme, LSP override with
  unknown `language_id` and no extensions, invalid
  `initialization_options` (TOML → JSON conversion failure).
- Warnings are printed to stderr before the alt-screen takes over,
  and the first is set as a startup status message via
  `Editor::set_status` (auto-clears on the first keystroke). If
  there's more than one warning, the status appends `" (+N more;
  see stderr)"`.

**File watching**: deferred to v2. v1 users relaunch to reload.

---

## 13. Package Manager & Registry

### 13.1 `arx install` Workflow

```
1. Resolve:  Query registry for metadata + source repo
2. Fetch:    Clone/download source
3. Verify:   Check signatures, audit dependencies
4. Build:    cargo build --release --target-dir ~/.arx/build/<name>/
5. Install:  Copy dylib to ~/.arx/extensions/<name>/
6. Load:     Notify daemon to hot-load
```

### 13.2 Registry

Git repository containing extension manifests:

```toml
# registry entry: git-gutter/manifest.toml
[extension]
name = "git-gutter"
description = "Show git diff indicators in the gutter"
repository = "https://github.com/example/arx-git-gutter"

[versions]
"0.3.0" = { sdk = ">=0.2.0, <1.0.0", sha256 = "abc123..." }
```

### 13.3 Trust Model

```rust
pub enum TrustLevel {
    Official,   // Signed with Arx project key
    Verified,   // Identity-verified publisher
    Community,  // Anyone can publish, source auditable
    Local,      // Installed from local path
    Sandboxed,  // WASM runtime regardless of publisher
}
```

- Native dylibs: full permissions. Default for Official/Verified only.
- WASM: sandboxed. Default for Community. Safe from untrusted sources.
- Dependency audit via cargo-audit / RustSec.
- Reproducible builds: registry stores source hashes.
- Lock file: `~/.arx/extensions.lock` pins versions and hashes.

### 13.4 Extension Development

```bash
arx ext new my-extension    # Scaffold
arx ext dev                 # Watch + hot-reload
arx ext test                # Headless test
arx ext publish             # Publish to registry
```

---

## 14. Project Management

### 14.1 Project Model

```rust
pub struct Project {
    pub id: ProjectId,
    pub root: PathBuf,
    pub detected_by: Vec<String>,
    pub config: ProjectConfig,
    pub file_tree: Arc<RwLock<FileTree>>,
    pub lsp_servers: Vec<LspServerId>,
    pub agents: Vec<AgentId>,
    pub vcs: Option<VcsState>,
}
```

### 14.2 Detection

Walk up from file path, matching markers: `.git`, `Cargo.toml`, `package.json`, `pyproject.toml`, `go.mod`, `.arx` (highest priority), `Makefile`.

### 14.3 Workspace

Groups multiple related projects. Cross-project search scope. Workspace file: `<root>/.arx-workspace.toml`.

### 14.4 Commands

```
project:find-file    C-c p f    Fuzzy file finder (ripgrep)
project:grep         C-c p g    Ripgrep across project
project:switch       C-c p p    Switch between projects
project:symbol       C-c p s    Symbol search (LSP)
project:recent-files C-c p r    Recently opened files
project:run          C-c p !    Run project command
project:test         C-c p t    Run tests
project:build        C-c p b    Build
project:tree         C-c p T    File tree sidebar
```

---

## 15. Keybinding & Input System

- **Keymap tree** with inheritance: `global → mode → buffer-local → overlay`
- Emacs-style key sequences (`C-x C-f`), Vim-style modal (as extension), chords
- **Which-key**: show available continuations after prefix key
- **Kitty keyboard protocol** in TUI for unambiguous modifier detection
- **Command palette** (`M-x`) with fuzzy search across all registered commands

---

## 16. Performance Targets

| Metric | Target |
|--------|--------|
| Startup to first frame | < 100ms (cold), < 50ms (warm) |
| Client attach to existing daemon | < 10ms |
| Keystroke-to-render latency | < 8ms (one frame at 120Hz) |
| Open 1GB file | < 2s, no UI hang |
| Concurrent agent sessions | 10+ without degradation |
| Extension hot-reload | < 500ms |
| Memory baseline (empty) | < 30MB |

---

## 17. Platform & Build

- **Minimum Rust edition:** 2024
- **Platforms:** Linux (primary), macOS, Windows
- **Terminal requirements:** Kitty graphics protocol, Kitty keyboard protocol, true color, Unicode. Ghostty, Kitty, WezTerm all qualify.
- **Build:** `cargo build`. Single binary with optional feature flags (`--features gpu` for wgpu).
- **CI:** GitHub Actions, cross-platform matrix, property-based testing for rope/OT.

---

## 18. Development Phases

### Phase 1: Core (Months 1–3)
- Async event loop + command bus
- Rope-based buffer with text properties
- Basic TUI rendering (crossterm)
- Keymap system, command palette
- File open/save, basic editing operations
- Extension SDK v0.1 with hot-reload
- Daemon + TUI client architecture

### Phase 2: Editor Essentials (Months 4–6)
- Window splits and layout management
- Tree-sitter syntax highlighting
- LSP client integration
- Completion framework
- Embedded terminal (termwiz or vte-based)
- Undo tree
- Session management (attach/detach/list)

### Phase 3: Agent Integration (Months 7–9)
- Agent trait and adapter system
- Claude Code embedded terminal adapter
- Agent permission model and audit log
- Edit proposal UI (accept/reject diffs)
- Agent-aware command composition
- TTY bridge with OSC protocol
- OT for agent edit reconciliation

### Phase 4: Ecosystem (Months 10–12)
- Package manager and extension registry
- Git integration (magit-style)
- Org-mode / Markdown editing
- GPU rendering frontend
- Project management
- Remote access (TCP + TRAMP-style)
- Documentation and onboarding

---

## 19. Open Questions

1. **ABI stability**: Use `abi_stable` crate for dylib boundary (recommended) or C ABI with Rust wrappers?
2. **Collaborative editing**: Build CRDT into the buffer from day one, or defer? (Recommended: structure ops for CRDT compatibility but defer full protocol.)
3. **GUI toolkit**: `wgpu` directly (like Zed) or leverage `cosmic-text` + `wgpu` for compositing? (Recommended: cosmic-text for shaping + wgpu.)
4. **Vim emulation**: Ship built-in Vim mode extension or leave to community?
5. **Buffer edit_with**: Add `buffer.edit_with(|snapshot| -> Vec<Edit>)` for optimistic concurrency control (read-modify-write without interleaving)?
6. **Broadcast mode**: For many-client scenarios (teaching), add a "leader" mode where one client's view is mirrored to followers without per-client diffing?

---

## CLI Reference

```
USAGE:
    arx [OPTIONS] [FILES...]
    arx <SUBCOMMAND>

CORE:
    arx                           Open/attach to default session
    arx foo.rs bar.rs             Open files in default session
    arx -s <name>                 Attach to named session
    arx -s <name> foo.rs          Open file in named session

SESSION:
    arx new [-s name] [-d dir]    Create new session
    arx list                      List sessions
    arx attach [session]          Attach to session
    arx detach                    Detach current client (C-x d)
    arx kill-session <name>       Destroy session
    arx rename-session <old> <new>

DAEMON:
    arx daemon [-d]               Start daemon [daemonized]
    arx shutdown                  Stop daemon
    arx status                    Show daemon status

HEADLESS:
    arx exec <command>            Execute editor command
    arx eval <expression>         Evaluate and print
    arx query <what>              Query state (JSON)
    arx pipe                      Read commands from stdin
    arx wait <condition>          Block until condition met

REMOTE:
    arx -S <socket>               Connect to specific socket
    arx -H <host:port>            Connect to remote daemon
    arx forward <host>            SSH tunnel and connect

PACKAGES:
    arx install <extension>       Install from registry
    arx uninstall <extension>     Remove
    arx update [extension]        Update
    arx extensions                List installed

EXTENSION DEV:
    arx ext new <name>            Scaffold extension
    arx ext dev                   Watch + hot-reload
    arx ext test                  Run tests
    arx ext publish               Publish to registry

CONFIG:
    arx config build              Recompile Rust config
    arx config edit               Open config in Arx
```
