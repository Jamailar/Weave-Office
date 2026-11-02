# Upstream sources

Pinned snapshots used by Weave Office. Original licenses, notices and asset attribution remain in each source tree. Local UI changes add hosted editor surfaces; Weave Office owns application windows, file tabs, dialogs and lifecycle.

| Source | Repository | Commit |
| --- | --- | --- |
| WordCraft | https://github.com/storytold/wordcraft | 6e0f7e79edabf5904ecb73104e809e6f053d9d27 |
| GridCraft | https://github.com/storytold/gridcraft | a85b1aea3a8a3369581a749d8da041e4db014504 |
| DeckCraft | https://github.com/storytold/deckcraft | 82573331a8f2b87ccb536b9b66d55b90295f2111 |
| PdfCraft | https://github.com/storytold/pdfcraft | eea00a6186546762d11d857666249672bff3fcad |

Upstream source licenses are MIT OR Apache-2.0; individual third-party assets and dependencies retain their respective licenses. ArtCraft branding is not used as Weave Office branding or bundled in its application resources.

## 给后续 AI 的参考项目与接手说明

上表是本项目直接复用的四个 Craft 源码来源，Commit 是导入快照的基线，**不是当前上游最新版**。本地 `vendor/` 已包含 Weave 的适配修改，不等于未经修改的原始快照；不要用重新下载的上游目录整体覆盖它。

| 能力 | 上游仓库与固定源码 | 本地目录 | 主要复用层与接入点 |
| --- | --- | --- | --- |
| 文字 / DOCX | [WordCraft](https://github.com/storytold/wordcraft) · [固定版本](https://github.com/storytold/wordcraft/tree/6e0f7e79edabf5904ecb73104e809e6f053d9d27) | `vendor/wordcraft/` | `doc` 文档模型、`fonts`、`layout` 排版、`docx` / `formats` 读写、`engine` 命令和历史、`ui-egui` 编辑视图 |
| 表格 / XLSX | [GridCraft](https://github.com/storytold/gridcraft) · [固定版本](https://github.com/storytold/gridcraft/tree/a85b1aea3a8a3369581a749d8da041e4db014504) | `vendor/gridcraft/` | `model` 工作簿、`formula` / `functions` / `calc` 公式计算、`xlsx` 读写、`engine`、`ui-egui` 编辑视图 |
| 演示 / PPTX | [DeckCraft](https://github.com/storytold/deckcraft) · [固定版本](https://github.com/storytold/deckcraft/tree/82573331a8f2b87ccb536b9b66d55b90295f2111) | `vendor/deckcraft/` | `model` 幻灯片与形状、`fonts` / `text`、`render`、`pptx` / `format` 读写、`engine`、`ui-egui` 编辑视图 |
| PDF 阅读与编辑 | [PdfCraft](https://github.com/storytold/pdfcraft) · [固定版本](https://github.com/storytold/pdfcraft/tree/eea00a6186546762d11d857666249672bff3fcad) | `vendor/pdfcraft/` | PDF 对象与格式处理、`engine` 编辑命令、`render` 页面渲染、`platform` 落盘、`print` 打印、`ui-egui` 编辑视图 |

四者都是实际代码依赖，不只是视觉参考。根目录 `Cargo.toml` 和 `Cargo.lock` 决定正式应用使用哪些 crate、feature 和 git revision。共享库还包括 [craft-libs](https://github.com/storytold/craft-libs)、[craft-fonts](https://github.com/storytold/craft-fonts) 与 [craft-i18n](https://github.com/storytold/craft-i18n)；字体和翻译的固定输入、许可证及准备方法分别见 [字体说明](../docs/fonts.md)、[翻译说明](../docs/i18n.md) 和 `scripts/prepare-*.py`。不要因为名字相同就假定依赖版本已统一；升级任一编辑器时用 `Cargo.lock` 核对 craft-libs 仍只有一个 revision，skrifa/harfrust 与 egui 一致，vello_cpu 只有一份。

主流办公套件仅是交互与视觉体验参考，不是上述开源代码的来源；不能把上游的功能清单当作 Weave 的验收结果。

### 建议阅读顺序与代码定位

1. 先读根 [README](../README.md)、[PLAN](../PLAN.md) 和 [验收记录](../docs/acceptance.md)，确认产品目标、已实现范围和未通过事项。
2. 正式启动入口是 `src/main.rs`；`src/app.rs` 管应用外壳，`src/host.rs` 管工作区和文件生命周期，`src/editor.rs` 负责四种编辑器适配。`crates/workspace/` 管共享工作区状态。
3. 各编辑器的嵌入接口均在 `vendor/<项目>/crates/ui-egui/src/app/hosted.rs`。改动 vendor 前读对应 `AGENTS.md` / `CLAUDE.md`，同时遵守根项目的用户目标与架构边界。
4. 公共服务优先查 `src/office_files.rs`（原生文件选择）、`src/clipboard.rs`、`src/printing.rs`、`src/output.rs`、`src/theme.rs`、`src/ui_fonts.rs` 和 `crates/font-assets/`。专业解析、排版、公式或 PDF 对象处理再进入对应引擎。
5. `integration/` 是早期实验，不是正式桌面入口；`crates/pdf-backend/` 的存在也不代表桌面 PDF 已改走该封装。开发机上相邻的本地参考仓库目录只用于参考，正式构建不依赖它。

### 修改与升级边界

- 复用四种专业引擎，逐项统一可共享的应用级服务。不要启动上游的四个独立桌面程序，也不要恢复它们各自争夺窗口、全局设置、文件面板或退出事件的行为。
- 本地补丁不仅在 `hosted.rs`，还涉及字体、主题、文件请求、剪贴板、渲染、模态操作边界等位置。下文列出关键契约，但不是完整逐文件 diff；升级前应以固定快照为基线检查整个 vendor 子树并保留本地修改。
- 各 ui-egui 的默认 feature 面向独立 App：`updater`（上游自动更新）以及 WordCraft/PdfCraft 的 `all-locales`（全部界面语言）。根清单对四个编辑器关闭默认 feature，新增上游 feature 时要确认宿主是否需要。`vendor/pdfcraft/vendor/vello_{cpu,common}` 是带 PdfCraft 补丁的 0.2.0，经根 patch 作用于所有渲染方。
- 字体字节已单份共享，字体发现/缓存尚未统一；保存入口已统一，可靠落盘尚未统一；宿主自动保存和异常恢复尚未接上。不要再次写成“底层已经全部合并”。
- 构建产物和大型输入留在外置盘。常规验证使用 `python3 scripts/cargo.py test -p weave-office --lib --test shared_fonts --test hosted_views --test paint_continuity --test external_interop --locked --offline -j 3`，按实际改动选择测试；打包方法见 [macOS 打包说明](../packaging/README.md)。不要为了交接文档更新重跑整套引擎测试。
- 保留各上游 LICENSE、NOTICE 和资产署名；开源代码许可不等于所有品牌素材都可复用。Weave 使用自己的应用标识和图标。

## Hosted integration patches

The local `crates/ui-egui/src/app/hosted.rs` adapters retain each editor's document model and editing view while delegating application lifecycle and file operations to Weave Office. Root integration tests exercise these adapters; the upstream standalone applications are not launched by Weave Office.

- WordCraft and DeckCraft consume completed host picture selections before save/close checks. Word file-dialog services, Grid picture services and Deck picture inboxes are connected to the root native chooser by `src/office_files.rs`; results retain their originating document and request identity.
- GridCraft PDF/CSV/HTML export commands and DeckCraft export/template commands are queued to the host even when a caller supplies a path. The host currently supports shared PDF export; unsupported formats produce an error rather than bypassing its write policy.
- GridCraft host-tab deactivation retains cell drafts and their selection. Save still validates and commits the draft; a rejected value must not prevent the host's explicit discard/close transaction.
- WordCraft, GridCraft and DeckCraft accept one host clipboard service for structured copies and system HTML/text. Native clipboard identity and per-editor copy generations prevent stale sessions from restoring or clearing newer copies. WordCraft and DeckCraft publish a read-only preview before cutting, so a refused system write retains the selection and history. GridCraft synchronizes explicit cancellation and queues cross-workbook cuts for a host-coordinated two-session move. Both workbook snapshots are prepared before either live document changes; source cleanup and destination values/styles/internal formulas commit together, with independent single-step undo/redo. Queue identity, drafts, stale snapshots and closed documents are checked before applying. Full-block internal absolute and relative references follow the move; external dependencies, dynamic references, defined names, formula validation, conditional formats, tables/charts/sparklines/pivots are conservatively refused because external workbook links and their dependency rewrites are not implemented. Special paste transformations and insert-cut cells are refused with the cut retained. Save/export drain pending moves before producing output; closing cannot discard a queued move.
- PdfCraft advanced file requests, generated documents and background output use host-owned request and completion contracts. The host protects every open document path before writing exported copies.
- PdfCraft exposes task-completion polling separately from active input and styling. Weave Office polls inactive documents and prevents close/quit from dropping unfinished OCR, optimization, export, action or print results. Closing also settles implicit OCR deferred in Read mode while retaining recognized pages for the save decision.
- PdfCraft page-image context menus exclusively own their popup while open. The generic page menu remains available for text and blank-page clicks; it must not reuse the image popup's widget identities in the same frame.

These patches describe Weave Office's embedded mode and do not change the standalone projects' published parity assessments.
