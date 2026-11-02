# Weave Office 本机验收记录

日期：2026-11-01。环境为当前 macOS ARM64 开发机，使用自行创建的合成文件。

统一应用已能在一个窗口管理文字、表格、演示和 PDF，完成四种原生格式的编辑、保存和重开。以下记录覆盖已经得到的结果；复杂文件保真、系统输入法组合输入和部分高级公共服务仍在验证中。

## 自动验证

| 可复现验证入口 | 结果与范围 |
| --- | --- |
| `PDFCRAFT_SYSTEM_FONTS=0 python3 scripts/cargo.py test --lib --test host_integration --test pdf_host_file_dialogs --locked --offline -j3` | 共 52 项通过：42 项库单测、5 项四类型宿主集成、5 项 PDF 文件请求契约。42 项库单测包括 29 项既有命令、关闭保护、顶栏指针回放、四类型视图状态、原生文件选择及 Office 请求隔离测试，另有 2 项混合标签溢出/CtrlTab、1 项 inactive PDF worker 关闭保护和 10 项共享剪贴板测试。当时剪贴板测试覆盖私有 NSPasteboard、发布失败时保留来源、跨平台注记匹配、取消 cut；此历史运行早于下述跨工作簿原子移动实现。 |
| `python3 scripts/cargo.py test --manifest-path vendor/wordcraft/Cargo.toml -p wordcraft-ui-egui --lib system_clipboard::tests --locked --offline -j3` | 11 项通过：结构化片段、显式粘贴参数、HTML/RTF、无文字片段占位、写入失败时保持来源/历史/选区，以及成功剪切只发布一次并保留 tracked move。首次运行补齐了锁定的开发测试依赖。 |
| `PDFCRAFT_SYSTEM_FONTS=0 python3 scripts/cargo.py test -p weave-office --lib clipboard::tests --locked --offline -j 3` | 10 项通过：共享结构化剪贴板、私有命名 NSPasteboard、身份与代隔离、写入失败保留来源；跨簿 cut 在宿主接收前仅排队，两端均不先写。 |
| `PDFCRAFT_SYSTEM_FONTS=0 python3 scripts/cargo.py test -p weave-office --lib sheet_transfer --locked --offline -j 3` | 11 项通过（8 项真实 Host、3 项应用生命周期）：跨簿值/加粗/绝对公式、独立撤销重做、来源和目标草稿、来源关闭或修改、目标变更、保护/越界/不支持粘贴拒绝、取消/新剪贴板代/外部复制、关闭 busy 保护、旧关闭方案失效、后续命令前处理，以及立即保存后两份 XLSX 分别在全新 Host 重开。使用内存剪贴板，没有操作真实桌面剪贴板或 GUI。 |
| `python3 scripts/cargo.py test --manifest-path vendor/gridcraft/Cargo.toml -p gridcraft-engine --lib clipboard_transfer::tests --locked --offline -j 3` | 11 项通过：两端原子提交、值/样式/公式/图片/注释/链接/字面验证/合并区域、完整数组、独立撤销重做、目标暂存失败、保护/越界/过期快照及外链依赖拒绝；完整块内的绝对、相对和限定工作表引用均随块移动。 |
| `python3 scripts/cargo.py test --manifest-path vendor/gridcraft/Cargo.toml -p gridcraft-ui-egui --lib app::clipboard::tests --locked --offline -j3` | 9 项通过：宿主委托先于写入、Enter 不消费待处理剪切、无宿主事务时拒绝降级、显式外部文本隔离、取消及新旧副本隔离。 |
| `python3 scripts/cargo.py test --manifest-path vendor/deckcraft/Cargo.toml -p deckcraft-ui-egui --lib app::clipboard::tests --locked -j3` | 2 项通过，含键盘与菜单两条路径、文字选区和无文字图形；拒绝写入不改变内容/选区/dirty/undo/redo，成功只发布一次，空光标不发布旧片段。 |
| `python3 scripts/cargo.py test --manifest-path vendor/pdfcraft/Cargo.toml -p pdfcraft-ui-egui --lib hosted_background --locked --offline -j3` | 11 项通过：后台 OCR/导出/优化/Action/打印完成接收、Read 模式隐式 OCR 关闭收尾、保存选择与生成文档确认、worker 失败不永久 busy，以及不修改输入/字体/主题。使用合成任务结果与自有写入器，不代表真实 OCR 模型和打印机验收。 |
| `PDFCRAFT_SYSTEM_FONTS=0 python3 scripts/cargo.py test --test host_integration --test pdf_editing --test pdf_export --test pdf_export_guard --test pdf_generated_documents --locked --offline -j 3` | 本轮相应验证共 27 项通过：5 项四类型集成、6 项 PDF 编辑、5 项共享导出、4 项高级写入保护、7 项生成文档。覆盖只读目标、后台写入时的实时路径保护和生成结果确认。 |
| `PDFCRAFT_SYSTEM_FONTS=0 python3 scripts/cargo.py test --test hosted_views --test hosted_export_routes --test office_image_files --locked --offline -j 3` | 7 项通过：八个混合标签绘制及无障碍检查；两种上游的带路径导出仍留在宿主队列、不直接写文件；三种 Office 图片交付、取消和迟到结果隔离、保存重开后的字节保留。 |
| `PDFCRAFT_SYSTEM_FONTS=0 python3 scripts/cargo.py test --lib mixed_tab_round_trip_retains_real_selection_scroll_zoom_and_active_page --locked --offline -j 3` | 1 项通过：走实际宿主挂载、输入钩子、样式与四种真实编辑视图，设置非空选区、非默认缩放及实际滚动，混合往返两轮后保持一致；同时核对活动页、工作表和单元格。初始字体布局及导航动画稳定后才建立基准。 |
| `PDFCRAFT_SYSTEM_FONTS=0 python3 scripts/cargo.py test --test pdf_image_menu --locked --offline -j 3` | 2 项通过：阅读文字选区后进入编辑，原图右键只显示图片菜单；菜单中旋转命令实际改变图像位置矩阵且可撤销；阅读文字和编辑空白处仍有通用菜单。临时恢复通用菜单同时绘制的旧行为时，回放因旋转菜单被覆盖而失败，随后已恢复修复。 |
| `PDFCRAFT_SYSTEM_FONTS=0 python3 scripts/cargo.py test --test native_app_menu --locked --offline -j 3` | macOS 主线程独立验证通过：不创建窗口、没有 key window 时，AppKit 实际匹配隐藏的 ⌘Q 并只提交一次宿主关闭请求；无修饰键的 q 与 ⌘W 不被截获。文件面板作用域保留原生编辑菜单，结束后恢复同一个常驻菜单和仍有效的退出 target。此测试不用默认 Rust 测试线程执行 AppKit。 |
| `python3 scripts/cargo.py test --lib --test paint_continuity --test office_exports --test hosted_views --locked --offline -j3` | 本轮联合回归 58 项 library、2 项逐帧绘制、10 项 Office 导出及 1 项宿主混合视图全部通过。 |
| `python3 scripts/cargo.py test --test external_interop --locked --offline -j3` | 4 项通过：使用第三方办公软件实际新建并保存的 XLSX、DOCX 和两页 PPTX，验证编辑、另存及全新 Host 重开；Python 标准库 ZIP/XML 与引擎模型双重检查。样本来源和生成步骤记录于 `tests/fixtures/README.md`。 |
| `python3 scripts/cargo.py test --test native_app_menu --locked --offline -j3` | Native AppMenu 原位刷新后 Quit 标题的测试通过。 |
| 本轮语言与根快捷键回归 | 4 项共享语言测试及 6 条相关命令验证通过；新增根快捷键回归通过。`⇧⌘P` 曾被 `consume_key` 折叠为 `⌘P`，现按事件修饰键精确消费。 |
| `title_bar_new_picker` | 新建浮层分帧指针测试通过，覆盖锚定位置、点击外部收起、Escape 收起、选择类型，以及点击浮层时不拖动窗口。 |
| UI 字体与绘制回归 | 既有 2 项逐帧绘制测试已加入实际 UI 字体安装并通过；新增 UI 字体契约测试 1 项通过。最新 `--lib --test hosted_views` 的 60 项库测试及 1 项宿主混合视图通过，包含新建浮层、紧凑字体、长文件名标签及语言偏好。PDF 字体隔离首轮的 6 项编辑测试通过，表单与填充签名的后续检查另行记录。 |
| PDF 页面字体隔离 | `pdfcraft-ui-egui --test it forms::` 的 44 项表单回放、`--test it fill_sign::` 的 33 项填充签名回放全部通过，覆盖自动字号、旋转/缩放下的文字框和保存。表单字号上界的既有数学单测也通过。界面字体测试同时核对真实 PDF 正文字体族在合并 UI 字体后原样保留；不以界面字体替换页面内容。 |
| `WEAVE_ACCEPTANCE_DIR="$WEAVE_DEV_VOLUME/DevCache/WeaveOffice/acceptance" PDFCRAFT_SYSTEM_FONTS=0 python3 scripts/cargo.py test --test native_ui_files --locked --offline -j 3 -- --ignored` | 5 项通过：用全新宿主读取界面实际保存的四类文件、正文/新增图片的 PDF、原图替换与变换的 PDF、页面旋转与插页副本，以及三种 Office 插图文件；核对正文、公式、幻灯片标题、批注、实际图像像素、位置矩阵及逐页旋转。读取前后所有样本内容及修改时间不变。 |
| `python3 scripts/cargo.py test -p weave-workspace -p weave-pdf-backend --lib --locked --offline -j 3` | 3 项工作区测试与 1 项 PDF 后端测试通过，含关闭保护与保存版本保护。 |
| `PDFCRAFT_SYSTEM_FONTS=0 python3 scripts/cargo.py test --manifest-path vendor/pdfcraft/Cargo.toml -p pdfcraft-annot --lib --locked -j 3` | 77 项批注模块测试通过，含 Unicode 字体嵌入、窄框数字不被截断、TAB 外观与重开后继续编辑；原有 ASCII 用例保持通过。 |

四类型集成测试实际验证了编辑后的内容，覆盖保存重开、同类型标签独立撤销、同路径及符号链接复用、真实写入失败，以及阻止覆盖其他已打开文件。三种 Office 的图片请求会保留原标签与请求标识；取消、切换和源标签关闭后的迟到结果不会把图片插到其他文档。文字和演示的保存检查会先接收已完成的图片选择，避免紧接着保存或关闭时漏掉图片。

表格标签切换保留单元格草稿，不再等同于按 Enter 提交。三项实际宿主挂载测试覆盖 Stop 验证拒绝的 `Blue` 草稿及其选区和光标、关闭扫描中的不保存，以及取消后保留全部标签；保存仍拒绝非法值且不创建文件。文本框的外部点击提交沿用上游行为，这些单元格测试不代表文本框也已保留全部编辑状态。

宿主剪贴板保留 Word 片段、Grid 单元格快照及 Deck 图形/页面/媒体，文档各自维护 Session 和撤销。测试验证跨标签的加粗、公式/样式及无文字图形，外部再次复制相同文字也会使旧结构失效；Word 到 Deck 的 HTML 加粗可保留。macOS 使用 `NSPasteboard.changeCount`，私有命名 pasteboard 的实际读写测试没有替换用户剪贴板。无系统序号的分支使用唯一 HTML 注记及完整 HTML/文本匹配，只有内存模拟验证，未代表 Windows/Linux 实测。Word/Deck 写入失败保留来源；取消 Grid 剪切不会恢复旧快照。跨工作簿普通剪切粘贴现由宿主协调两份独立 Session 原子移动；目标接收值/样式/块内公式后，来源清空，各自保留独立的一步撤销/重做。尚不能建立外部工作簿链接：块内引用块外、块外公式引用块内，以及动态引用、名称、公式验证、条件格式、表格/图表/迷你图/透视表等未实现依赖重写的来源簿会保守拒绝并保留剪切。仅完整普通粘贴受支持；选择性粘贴的转换选项与插入剪切单元格明确拒绝。来源/目标草稿、来源快照或目标状态变更、关闭、保护、越界均不会产生半次移动。取消、新剪贴板代和外部复制会丢弃迟到请求；保存/导出先处理队列，关闭不会丢弃队列，旧关闭批准在移动结果到达后失效。

所有 PDF 标签的后台完成结果由宿主轮询，独立于活动编辑器输入和字体/主题。真实 worker 测试在另一文字标签活动时尝试关闭和退出，任务期间保留两份文档，结束后结果仍归原 PDF；这项是无窗口生命周期验证，不代表实际 OCR 模型、打印机或所有高级工具验收。

PDF 测试还核对 FreeText 批注的内容、作者、几何位置、撤销重做、保存前提交 TypeBox，以及未发布评论草稿的保留与保护。

共享导出测试实际读取输出 PDF 的页面和文字，并检查原编辑器路径、未保存状态、撤销能力未改变。已打开的目标文件及其符号链接不能被导出覆盖。后台写入器在真正写入时读取当前文件身份；测试覆盖其创建之后发生的打开、另存为、关闭，以及已启动 worker 的策略变化。提取为新文档先创建宿主标签再确认源页删除，失败、取消、源修改或新草稿均保留源页；关闭事务中收到生成结果会取消旧关闭计划。

中文 PDF 测试使用构建时校验的开放字体，关闭系统字体读取。它们验证真实字形像素、保存重开、继续编辑及字体资源复用；不把数据中保留中文字符串当作字形正常的证据。

## 打开/编辑时短暂黑屏：修复与验证

本轮用户确认在打开文件、切换页面或开始编辑时短暂闪黑后恢复。已复现编辑器提交空帧的路径并修复，更新安装包后正常重启；用户是否仍能遇到其他闪黑路径，待后续实际使用观察。

### 已证实的原因

- Word hosted UI 在 `fonts_frames < 2` 时返回空界面；Grid 在 `fonts_ready` 或需要 `rebind` 时返回空界面；PDF 在 `!fonts_ready` 时返回空界面。对应帧宿主只画工作区 shell，编辑区未提交绘制。
- eframe 默认清屏色为半透明深灰 `(12, 12, 12, 180)`；编辑区空帧因此显露为黑色/深色，造成闪黑后恢复的观感。
- Grid 的 `used_fonts` 未去重；普通 fill/border 样式变化也会触发字体重绑，使 Grid 更容易遇到空帧门控。

### 当前实现与自动验证

- Word、Grid、PDF 改用 `ctx` 中实际可用的字体状态，移除固定帧数空等；Deck 在绘制 shell 前准备字体，仅在确实可以重跑时丢弃当前 pass，末 pass 使用安全不透明 fallback。
- 宿主每个 pass 开始时执行 `mount_active`；保留 raw input hook，以免修复绘制连续性时破坏 IME 输入处理。宿主底面改为不透明，`clear_color` 与当前主题匹配。
- 新增 `tests/paint_continuity.rs` 两项测试，分别覆盖最多 1 个及 2 个 pass；检查四类编辑器首次挂载与恢复混排时，每个提交帧都有画布和至少 10 个按钮。测试不铺宿主 root 背景，以免背景掩盖子界面的空绘制；Grid 用例还逐帧变更中文、fill、border、Arial/宋体字体。
- 后续联合回归 `--lib --test paint_continuity --test office_exports --test hosted_views` 全部通过：58 项 library、2 项逐帧绘制、10 项 Office 导出及 1 项宿主混合视图。外部文件互操作现有独立 4 项测试，见下文。
- 临时恢复旧 Word 两帧 gate 后，相同逐帧测试按预期失败：首个提交帧只有 3 个 shape，没有编辑器画布。随后恢复修复源码并再次通过两项逐帧测试。负向对照日志位于外置盘 `acceptance/black-flash-negative-control.log`。
- Deck 原有 `a_control_click_on_the_first_font_discard_is_requeued_and_lands` 测试通过，确认字体准备重跑时的合成点击仍能到达控件。该测试不替代真实系统输入法候选测试。

### 外部来源工作簿小样本

本轮自建并保存 `tests/fixtures/external-origin-pricing.xlsx`，未改动真实用户文件；自建样本已关闭。`external_interop` 的 1 项测试通过，覆盖该小样本中的中文、粗体宋体 11 pt、日期序号与格式及 4 个公式，其中修改 B2 为 4 后，公式结果核对为 60 和 322.5，并在全新 Host 保存重开。此证据只说明这一个独立小样本，不代表复杂工作簿兼容性。

### 外部来源原生文件互操作与界面语言

- 当前 `tests/external_interop.rs` 的 4 项均通过。第三方办公软件新建保存的 XLSX、DOCX 与两页 PPTX 由 Python 标准库 ZIP/XML 解析和引擎模型双检，覆盖编辑、另存、全新 Host 重开。DOCX 中文标题居中、深蓝、粗体、红色、段距及 2×2 底纹表保留；PPTX 未编辑另存后精确保留两页几何信息。
- 编辑 PPTX 的 `AutoFit::Shape` 文本框后，高度由 660400 变为 655320 EMU（约 -0.4 pt），属于源 `spAutoFit` 允许的文字重排；引擎模型与 XML 检查一致，其余文字 run、顺序、尺寸和填充保留。此样本不代表复杂全格式保真已完成。
- 统一公共界面语言偏好已实现 `zh-hans` / `en`。Word Language Preferences pane 保留，只显示校对状态且为只读；切换界面语言不会改变文档校对语言、dirty 状态或 undo 历史。
- PDF 切换语言后的字体优先顺序已修复。旧条件负对照按预期失败，确认该回归能被测试捕获。
- Native AppMenu 原位刷新 Quit 标题测试通过。根快捷键曾将 `⇧⌘P` 误折叠为 `⌘P`；现按事件修饰键精确消费，并有新增回归覆盖。

### 新建浮层与 UI 字体实现

- 顶部“+”按钮下方锚定新建浮层已实现：原创图标依次为文字、演示、表格、PDF；选择后创建对应类型标签，支持点击外部、Escape 或再次点击“+”收起。浮层跟随按钮定位，不居中，也不提供未支持的文件格式或云服务。
- `title_bar_new_picker` 实际分帧指针测试通过，验证锚定、关闭方式、所选类型和不触发窗口拖动。最终安装包另已通过 GUI 分别新建文字、表格、演示和 PDF；再次点击“+”收起、选择后浮层消失均有实际操作证据。
- 新增 `src/ui_fonts.rs`：运行时读取 macOS SF 与 PingFang 字体，Inter/Noto 作为内嵌 fallback；Apple 字体不随应用打包。当前实现采用共同 24 pt 控件、6×3 padding、约 32 pt 导航尺寸，以及 Word/Sheet/Deck 分别 84/88/78 pt 的 ribbon 尺寸。这些是本轮实现参数，供界面统一使用。
- hosted Word 隐藏组 caption，同时保留完整控件 launcher 与无障碍组名；导航未选中项改用正常字重，选中项略加重。Word/Deck 文档 renderer 字体体系独立；PDF 的正文预览、表单、填充签名、水印及签名外观使用独立 `pdf-editor` 字体族。上述字体隔离与逐帧测试通过。
- GUI 已从 Word 审阅的“语言首选项…”进入公共语言页，显示原英语校对状态；切换 English 后公共壳和设置改为英文，校对状态保持，再恢复中文。演示 `⇧⌘P` 已打开命令面板，通过原 Export 设置选择 Outline，再由系统 Save 面板写出 `weave-deck-ui-outline-20261101.txt`；独立读取确认两页中文完整（91 字符），源 PPTX SHA-256 未变。
- 界面截图保存在外置盘 acceptance 中：`weave-new-picker-20261101.png`、`weave-density-{word,sheet,slides,pdf}-20261101.png`、`weave-density-save-dialog-20261101.png` 和 `weave-language-settings-20261101.png`。这些验证覆盖本轮指定界面，不代表所有专业弹窗或导出格式均已 GUI 逐项验收。

### 外部来源 DOCX 宋体与 macOS 显示比例修复

前轮诊断确认普通正文没有被误导入为粗体，UI 的 SF/PingFang 也没有进入正文 renderer。问题来自缺失宋体落入通用 Sans、中文片段按字符覆盖混用字体，以及较高的替代字体行框让源 15.6 pt 文档网格取到 31.2 pt；另有 hosted 画布的 `96/72` 比例，使标称 100% 纸宽约为预期值的 4/3。

本轮改为显式宋体家族替代，优先读取已安装的中文宋体，再使用开放的简体中文衬线字体；EA 请求也保持同一命名字体。只有缺失宋体命中已知替代时，排版使用该替代字体合法的 OS/2 typographic body，不附加 leading。已安装字体、其他 glyph fallback、字号和网格取整规则保持原逻辑，20 pt 文字仍占多行网格。样式缓存同时区分最终度量，避免同一物理字体通过不同请求使用时相互覆盖。

- Word native 构建增加独立内嵌的 Noto Serif CJK SC Regular（OFL-1.1），固定字体、清单和许可证均从外置盘校验，未复制第三方办公软件或 Apple 字体。无系统字体数据库测试确认实际内嵌字节可绘制中文、英文和数字；UI 的首个 Hans 字体仍是 Noto Sans。
- hosted macOS 画布改用每文档 pt 对应一个逻辑屏幕 pt 的基准；其他平台及 standalone 原比例保留。两项 `hosted_screen_` 测试通过，覆盖 2× backing pixels 下的纸宽、100/150% 切换、Fit 往返、标尺、hit、caret 和 IME rect；文档模型和打印 layout 不变。这不替代真实输入法候选操作验收。
- `wordcraft-fonts --lib` 为 108 项通过、1 项因缺少另行授权的 kaiu.ttf 而按原规则忽略；其中包括新增内嵌字体测试。宋体 layout 3 项和 EA 混排缓存 1 项通过；公共宿主 60 项、逐帧绘制 2 项、外部文件互操作 4 项通过。
- 外部办公软件从同一自建 DOCX 原生输出基准 PDF；PyMuPDF 与 pdfplumber 独立交叉核对字形尺寸、baseline 与向量边框，两个 PDF 均由 Poppler 以 144 dpi 渲染并人工查看。同为 595.3×841.9 pt 纸张，新 Weave PDF 首段、强调段、两行表格及尾段 baseline 比基准低约 0.38–0.48 pt，标题低 0.731 pt；此前的整行网格翻倍已消除。宋体字形、标题中点间距及其他字体细部仍有差别，不据此声称完整 DOCX 视觉保真。

基准与新版 PDF、PNG、layout JSON、独立 measurement/comparison JSON 均保存在本机验收目录，不随仓库发布。诊断临时 Rust example 已删除，两个 DOCX 原件 SHA-256 均保持不变。

### 表格边框 band 修复

表格行高已包含顶部边框全宽，以及末行底部边框全宽；原实现却把描边中心放在预留区域的外沿。现将水平线移入自己的 band，末端竖线收至底部 band 上沿，保留文字、行高和分页 flow。`CellPart` 明确记录本层边框，跨页分片重建这些索引，避免把嵌套表格线误认为父边框；单侧共享边回借的顶部 inset 也保留到续页。

- 完整 `wordcraft-layout --lib` 577 项、`wordcraft-render --lib` 65 项通过。新增 8 项参数化回归覆盖三种边宽、Auto/AtLeast/Exact 行高、单侧边、合并、连续跨页与真实嵌套表格。
- 重新从同一外部来源 DOCX 导出 `weave-word-final-20261101.pdf`，由 PyMuPDF 独立读取并用 Poppler 渲染检查。三条水平线中心为 176.775 / 202.125 / 227.475 pt，与基准各差 0.015 pt；列边界不变。最终 PDF/PNG、layout JSON 及 comparison JSON 均在外置盘 acceptance。
- PDF 中的水平边框中心间距不等于实际 flow 行高，不能据此把末行含 bottom band 的高度强行缩短。不同可见边宽的 OOXML 冲突优先级仍是既有限制；本次对此保留原有中心叠加，并使用线性区间扫描检测，未宣称完整解决所有边框冲突。

### 安装与原生界面复验

修复已打包至 `~/Applications/Weave Office.app`，Rust 格式检查及严格签名检查通过。闪黑修复那轮正常重启后恢复 K16；本轮新建浮层及字体更新前确认表格已保存，通过 ⌘Q 正常退出。最终版本的四类界面、浮层、语言和演示提纲导出已按上述范围 GUI 复验。结束时仅保留原 `weave-image-sheet.xlsx`，恢复本轮开始时的活动单元格 L20，中文偏好和浅色 1× 界面缩放保持；没有修改原文件。未用桌面高帧率录像证明所有操作均无闪帧；系统输入法与持续原生拖动仍待用户体验。

2026-11-02 续轮完成宋体、屏幕比例、表格边框与屏幕字重修复后的安装和复验。最终 arm64 Mach-O UUID 为 `D24D7A6C-2250-34DA-994A-6818FA7406A3`，与外置 target 的构建相同，严格 codesign 校验通过；文档衬线字体 OFL、清单和 NOTICE 已纳入包。全程正常 ⌘Q 重启，没有强制终止或丢弃未保存内容。

更新开始时实际工作区与前轮记录不同：表格活动位置为 J9，另有未保存的 `未命名PDF4`。先由原生 Save 面板将 PDF 保留为外置盘 `weave-preserved-pdf4-20261101.pdf`，再正常更新。最终恢复表格 J9、PDF 编辑模式、注释侧栏与 62% 显示；只保留这两个干净标签。表格、保留的 PDF 和外部来源 DOCX 原件 SHA-256 在验证前后均一致。一次自动化 AX 点击名称框没有获得键盘焦点，导致位置文字落入单元格，已立即 Undo 并确认恢复干净状态，未保存改写；最终重启后改用实际格子点击恢复 J9，也清除了这次自动化的 redo 历史。

### 屏幕字重与保存点边界

- 实际界面复验发现：即使普通正文 `bold=false`，macOS screen render 仍附加轮廓描边，11 pt / 2× 下每边约 0.283 px，造成屏幕伪粗。hosted 文档页现关闭这层额外加重，保留字体本身轮廓、真实粗体及 standalone 原策略；页面纹理 key 也区分 hosted 状态。
- 新屏幕与原屏幕同一正文、尾段和表格区域的墨量约减少 34–36%，普通与粗体可清楚区分，人工查看确认宋体细横画恢复。新图比基准仍轻约 13–14%；所用宋体轮廓与栅格器不同，不以墨量单一指标推断新的格式错误，也不重新硬编码加粗。原图、最终界面图和 ROI 方法记录为 `weave-word-screen-20261101.png`、`weave-word-screen-final-20261102.png`、`screen-weight-comparison-20261102.json`，均在外置 acceptance。
- `wordcraft-ui-egui hosted_` 首次发现一个既有保存点回归失败，exact 单跑也复现：成功保存未结束 Typing 合并组，后续输入仍并进保存前的撤销项。成功持久化并确认保存当前文档时现调用 `close_typing()`；失败保存和异路径有损副本不改变分组。原回归新增 Undo 后全文为 `saved`、Redo 后为 `saved new work` 的内容断言，避免仅绕过 dirty 检查。
- 修复后 hosted 14 项通过（包含屏幕字重、100/150% 坐标与保存点）；引擎普通保存/副本边界及失败保存 stamp 两项分别通过。最终公共宿主 60 项、混合视图 1 项、逐帧绘制 2 项、外部文件互操作 4 项再次通过，日志为 `weave-font-final-tests-20261102.log`。
- 已安装应用中原生另存自建副本 `weave-word-savepoint-ui-20261102.docx`，粘贴并保存标记 A，继续粘贴 B，再 Undo 回到 A 且清除修改标记；Redo 恢复 B 和修改标记，再 Undo 后正常关闭。独立读取落盘 DOCX XML 确认只有 A，没有未保存的 B。该界面回放验证保存/编辑/撤销路由；连续 Typing 合并边界由前述引擎与 hosted 回归验证。原外部来源样张字节未改变。

## 已完成的原生界面操作

通过打包后的 `Weave Office.app` 操作，未启动四个上游独立应用。

| 场景 | 实际结果 |
| --- | --- |
| 混合标签 | 同一窗口同时出现 DOCX、XLSX、PPTX、PDF 和第二份 DOCX，五个文件各自有标签。 |
| 标题栏与工具栏 | 新版保留真实原生 titlebar，Accessibility 中可见原生 close/fullscreen/minimize 控件；顶部混合标签，第二行整合 File、打开/保存/打印/撤销/重做图标和专业导航。界面截图与 AX 原生控件证据分开记录。 |
| 旧版标题栏记录 | 早期构建移除了系统标题栏，以三个自绘窗控替代；Weave 菜单、新建/打开与文件标签共用顶行，标签溢出和最大化/还原曾实测通过。该记录仅适用于旧版，不代表当前窗口控件实现。 |
| 五个混合标签与图标 | 同一窗口显示 Word、Word、Grid、Deck、PDF 五个自建文档标签，四类图标各不相同。 |
| 工具栏命令 | 共享 Word undo/redo 按钮在实际 Word 编辑后操作成功；Grid/Deck 功能区卡片已改为平面样式，PDF 模式状态使用文字下划线指示。 |
| 原生窗口关闭 | 点击原生 close 控件后出现未保存 Word 确认；选择取消后窗口和文档保持打开。 |
| 系统打开入口 | 新版工具栏打开图标调出 macOS `NSOpenPanel`，取消后返回编辑器。 |
| 统一保存确认 | 对本轮自建 Word 草稿按 ⌘W，显示新式确认框；取消后正文保留。点保存后打开原生 `NSSavePanel`，取消面板回到同一个自建确认框，文档内容仍保留。按钮顺序为左侧红色“不保存”、描边“取消”、最右蓝色“保存”。 |
| 文件载入错误 | 用自建无效文件 `weave-ui-invalid-20261101.docx` 触发 Weave 自建载入错误提示；右下主操作可关闭提示，原有 dirty Word 标签仍在。错误样本为本轮自建，未修改用户文件。 |
| 打印窗口 | 新式 footer 布局实际打开 macOS 打印窗口；系统发现真实打印机。没有提交打印任务，关闭打印窗口后正常返回。 |
| 对话框样式实现范围 | 宿主保存确认、操作错误、文件错误/重试和打印统一由 `src/dialog.rs` 实现。内容宽 420 pt、内边距 20 pt、圆角 8 pt；footer 与内容分隔，按钮高 32 pt、最小宽 84 pt。四个 vendor 的公共窗口 frame 与 OK/Cancel helper 也采用相同尺寸和间距；PDF prompt 局部按钮变体保留 `PromptButton::ORDER` 及安全默认语义。Grid 的宏停止、宏保存和动态问题提示也补齐卡片、footer 及主次按钮；动态多选保留原有顺序和默认选项。专业属性表单没有改写。以上是代码实施范围，GUI 只覆盖本表所列场景，并非所有 vendor 专业弹窗都逐一验收。 |
| 系统打开面板 | ⌘O 显示 macOS `NSOpenPanel`；通过系统目录导航多选五份混合文件并打开，系统“前往文件夹”字段可定位完整 PDF 路径。 |
| 系统保存与覆盖 | 新建文字文件保存为 `weave-native-picker.docx`，另存为也显示 `NSSavePanel`；同名目标的覆盖确认由原生保存面板显示系统 Replace/Cancel，取消后原文件保留。重开及独立读取 DOCX XML，均得到两段预期中英文。 |
| PDF 的文件与目录面板 | 图像工具显示系统文件面板；临时插入空白页后，拆分显示系统文件夹选择器。取消不输出文件，随后撤销插页恢复一页及干净保存点，源文件未保存改写。 |
| 三种 Office 的插图面板 | 文字的插图、表格图片配置中的 Browse、演示的图片入口均打开 macOS 原生选择器。选择自行生成的 PNG 后立即通过原生保存面板保存；独立读取 DOCX/XLSX/PPTX 的 ZIP/XML，各有一个图片对象，媒体字节与源 PNG 的 78 字节完全一致。关闭三个标签后，从系统打开面板多选重开，三个标签均为干净状态，文字视图仍包含图片对象。 |
| 面板期间的菜单退出 | 原生文件面板中点击“退出 Weave Office”，面板结束后进入统一保存提示；取消退出保留原文和未保存测试尾文。之后撤销测试尾文恢复保存点。 |
| 最小化后的退出与恢复（旧版自绘按钮） | 旧版中打开已保存 Word 和新建未保存 PDF，激活 Word 后点击自绘最小化按钮；⌘Q 恢复窗口并定位 PDF 保存提示。取消后两个标签仍在。此项未验证新版原生最小化按钮后的退出恢复。原生文件面板期间的菜单退出及其取消后普通 ⌘Q 有效也已通过。 |
| 文字编辑 | 粘贴三段中文与英文，保存为 DOCX；切换经过其他三种类型后，文字内容仍完整显示。 |
| 表格编辑 | 粘贴中文 A1:D3 数据及 `=B2*C2`、`=B3*C3` 公式，保存为 XLSX；独立重开后结果分别为 30、60。 |
| 演示编辑 | 在标题占位区粘贴 `Weave Office 统一办公`，画布与缩略图显示中文，保存为 PPTX。 |
| PDF 编辑 | 添加 `Reviewed amount: 123.45` 评论并提交，保存为 PDF；关闭标签并从统一打开入口重开，评论面板仍显示这条批注。 |
| PDF 原页面内容编辑 | 自建含文字和图片的 PDF，在页面中将 `Weave source invoice 123.45` 改为 `Weave edited invoice 678.90`，通过图片入口添加不同配色的 PNG；另存、关闭、重开后页面显示新文字和图片。独立读取确认没有批注，原图片保留，图片数由 1 变为 2，源文件原文保留。 |
| PDF 已有图片编辑 | 通过原图右键菜单和系统原生选择器替换为自行生成的 PNG，顺时针旋转 90°，再以 Shift+方向键向右、向下各移动 10 点；另存为 `weave-image-edited.pdf`，关闭重开后方向与位置保留。独立 pypdf 解析确认只有一次图像绘制、像素与替换 PNG 一致、矩阵为 `[0,-120,80,0,66,170]`，原正文及源文件 SHA-256 不变；Poppler 独立渲染外观正常。增量保存保留未再绘制的原图资源，不把资源表条目数当作页面图片数量。 |
| PDF 页面编辑 | 通过编辑面板实际旋转原页面，再经系统原生面板选择另一份自建 PDF 插入；定位第二页删除，再使用统一撤销恢复。另存为 `weave-pages-edited.pdf`，关闭重开后仍为两页，页面旋转分别为 90°、0°，两页正文和各自图片完整。独立 pypdf、Poppler 及全新宿主读取均通过；此处修改的是文档页面旋转，区别于右侧工具栏的临时视图旋转。 |
| 两份同类型文件 | 第二份文字文档中输入独立中文内容；切回第一份，原文保持不变。 |
| 取消退出 | 有未保存内容时按 ⌘Q，统一提示保存、取消、不保存，默认聚焦保存。按 Escape 后全部标签保留。 |
| 退出取消与批准 | 五个混合标签中按 ⌘Q 选择取消后，五个标签完整保留。随后对两个有修改的 Word/PDF 标签逐个选择不保存后，首次批准退出即使应用进程结束。该结果验证本次退出路径，不推断旧问题的根因。 |
| 对话框阻止编辑 | 保存提示显示期间输入测试字符，取消后正文没有收到这些字符。 |
| 保存点 | 撤销临时输入后，正文恢复保存内容，文件标签的未保存标记消失。 |
| 正常退出 | 已保存文件可通过 ⌘Q 正常退出；新版在批准未保存项后进程结束。 |
| 窗口关闭取消（旧版自绘按钮） | 旧版点击自绘关闭窗口按钮后，统一流程定位未保存的新建 PDF；Escape 取消后三个标签仍保留。新版原生 close 控件及取消流程见“原生窗口关闭”。 |
| 深浅主题（此前版本） | 旧版自建标题栏、控件和设置窗口的深浅主题实际切换可读；验收后恢复原来的浅色偏好。 |
| 跨编辑器剪贴板 | 从文字文档通过 ⌘C 复制两段中文，再在表格通过 ⌘V 粘贴，分别进入 A1/A2；来源文字保持不变。 |
| 中文主界面与演示切换 | 最新开发包中的四种编辑视图均可显示中文主界面；演示功能区与状态栏可切入，无障碍启用时未再出现已修复的锁重入。个别上游控件仍有英文。 |
| Finder 文件入口 | Finder 的“打开方式 → Weave Office”启动应用并打开 DOCX；重复执行仍只有原来的一个文件标签。未改变文件默认关联。 |

GUI 文件保存在外置盘专用目录 `$WEAVE_DEV_VOLUME/DevCache/WeaveOffice/acceptance/`。它们是本机验收输出，不是用户原文件，也不作为上游兼容性样本库发布。

新版界面截图由自建文档生成，分别为 `weave-ui-word-20261101.png`、`weave-ui-sheet-20261101.png`、`weave-ui-slides-20261101.png` 和 `weave-ui-pdf-20261101.png`。截图左上角包含截图工具提供的紫色系统共享叠层，该叠层不是应用绘制内容；原生 titlebar 控件存在性以 AX 观察单独验收，不以截图像素作为证据。

本轮对话框截图为 `weave-ui-save-dialog-20261101.png`、`weave-ui-print-dialog-20261101.png` 和 `weave-ui-error-dialog-20261101.png`。

此前对话框验收结束时仅丢弃该轮创建的 Word 草稿，应用回到空白 home；没有改动用户文件，也没有提交打印任务。本轮闪黑修复后的运行状态见上文，已恢复原先打开的表格。

中文粘贴、显示与保存已经验证；这些结果不替代操作系统输入法的组合、候选选择、提交与取消测试。

系统文件面板内的 ⌘V 和 ⌘Q 在本轮自动化中未触发预期动作，尚需区分面板事件处理与自动化投递。完整路径输入、系统覆盖确认及面板菜单退出已分别验证，不将它们记作上述两个快捷键通过。

顶栏的 egui 跨帧指针回放已经验证标签重排、窗口空白拖动、双击最大化/还原和三个窗控路由。桌面自动化的快速拖动未形成标签重排；依赖层会忽略在同帧中完成的拖动，新增分帧测试复现了这一边界。这些回放不替代用户稍后体验实际鼠标拖窗和拖标签。系统输入法组合输入也待用户后续体验。

## 构建与交付进展

- 最新优化 release `.app` 已完成打包、启动及本轮指定 GUI 复验，位于 `~/Applications/Weave Office.app`；严格签名检查和格式检查通过。正式发行签名、公证和最低系统版本仍需对应环境验证。
- 本次打包包含新版工作区布局和原创应用/文档图标；图标经 `iconutil` 检查，bundle 设置了应用及各文档类型图标引用。新版 GUI 操作和四类界面截图见上文。旧版“原运行进程未重启、新逻辑下次启动生效”的记录仅适用于当时共享剪贴板/后台任务那次交付，不代表当前安装包状态。
- Root 保存确认、操作错误、文件错误/重试和打印窗口已统一实现并再次打包、启动。修改后的 53 项单元测试通过；最终四个 vendor 的窗口 frame/helper 修改后，bundle build、Rust 格式检查及签名检查通过。PDF 原有三项 prompt 键盘选择测试随后通过（`--manifest-path vendor/pdfcraft/Cargo.toml -p pdfcraft-ui-egui --lib prompt_button_tests`），覆盖默认 Save、方向键边界、Tab 循环及按钮对应答复。GUI 只覆盖上表中的保存、打印及文件载入错误场景。
- 对话框版本替换前确认旧版应用显示无文档 home，通过 ⌘Q 正常退出。后续闪黑版本更新时确认表格已保存，正常重启后重开同一文件并恢复 K16，未修改文档内容。
- Cargo 目标位于 `$WEAVE_DEV_VOLUME/DevCache/WeaveOffice/target`，默认应用包位于其中的 `bundle/`。构建脚本验证外置盘挂载和输出目录，不自动回退系统盘。
- 通过 `CARGO_TARGET_DIR=$WEAVE_DEV_VOLUME/DevCache/WeaveOffice/rebuild-20261101/target CARGO_BUILD_JOBS=3 python3 scripts/cargo.py build --bin weave-office --locked --offline -j3` 从此前不存在的 target 编译全部依赖与主程序，用时约 5 分 28 秒，生成程序输出 `Weave Office 0.1.0`。后续应用级退出入口改动也在该目录重编通过。此项复用已下载的 Cargo 源码及固定字体/翻译输入，证明空编译缓存重建，不等同于新机器的联网安装或旧版 macOS 验证。
- `--replace` 只替换同一 bundle ID 的生成应用；先完成新包，提交失败时恢复旧包。构建失败时旧包保留。
- 开放字体与翻译输入固定版本并校验，位于外置盘；字体 SHA-256、许可和翻译目录检查已接入开发及打包脚本。
- 独立运行使用位于用户 Applications 的应用包，在 `/tmp` 启动，移除字体与翻译输入环境变量，并用临时 sandbox profile 禁止读取项目源码、参考仓库、外置 `target`/`inputs` 和其他办公软件；已核对拒绝规则生效。该进程仍能新建并操作四种编辑器、显示中文、跨编辑器复制粘贴及处理关闭取消，最后正常退出。此项证明当前包无需读取这些开发资源，不代替另一台干净机器的兼容性验证。

## 2026-11-02 优化构建与界面密度复验

- 已安装优化 release 包 `~/Applications/Weave Office.app`，arm64 Mach-O UUID `3246D071-C889-373A-8189-578E88E5568A`，严格签名验证通过。首次 release 编译用时 13 分 40 秒；最后一轮格式窗口改动重编 49.42 秒。构建日志为外置 acceptance 中的 `release-build-20261102.log`、`release-compact-dialog-build-20261102.log`、`release-compact-install-20261102.log`。
- UI 使用公共 SF/PingFang 字体链与 13/11 pt 基础字号；常规控件高 24 pt，按钮 padding 6×3 pt，常规间距 6×4 pt。表格普通表单和演示命令搜索补齐显式卡片 frame（20 pt 内边距、8 pt 圆角）；表格格式窗口的六个页签及常用字段补充中文。这里只记录本次覆盖的字段，颜色选择器等子控件仍有部分英文。
- 在 1280×706 逻辑点窗口实际打开表格格式的六个页签并取消。数字格式两列各自最多 216 pt 高，右侧预设列表可滚至最后的 `@`，示例、自定义代码输入和底部“取消/确定”保持可见；取消后文件保持干净。截图为 `weave-format-cells-compact-20261102.png`。六个页签未逐一提交格式修改，这项验证布局与取消路径。
- 同一最终安装包的混合五标签中，文字样张中英文和表格、电子表格公式合计 292.5、两页演示的缩略图与正文、PDF 已编辑正文和图片均实际显示。顶部“+”显示锚定的四格式图标浮层，截图为 `weave-ui-final-new-20261102.png`。截图工具的紫色系统共享叠层不属于应用。
- 最终包从中立临时目录启动，子进程移除六个构建字体/翻译环境变量，临时 Seatbelt profile 禁止读取整个 外置盘、本项目、参考仓库及其他办公软件 bundle，并禁止写入五个应用偏好目录。源码、外置字体清单和该办公软件三处路径被拒绝、中立 DOCX 可读的对照验证已通过。五个文档均使用自己制作的中立副本，正常退出后副本 SHA-256 和真实 Weave 偏好 SHA-256 全部保持一致；证据为 `runtime-isolation-final-20261102.json`、`runtime-isolation-policy-controls-20261102.json` 和 `runtime-isolation-runtime-20261102.sb`。未禁止网络，不把这项记为网络离线、另一台机器或最低系统版本验证。

## 2026-11-02 最终包四格式保存重开及原生退出补验

使用同一个 release 可执行文件（UUID `3246D071-C889-373A-8189-578E88E5568A`）完成一轮 GUI 修改、保存、正常退出、全新进程重开；工作文件均为自行制作样本的副本。证据位于外置 acceptance 的 `release-roundtrip-20261102-y9p90pw9/`，独立结果记录为 `independent-readback.json`。

| 类型 | 本轮实际修改及保存重开结果 |
| --- | --- |
| DOCX | 在原外部来源样张前加入 `Release 中文验收 2026.11` 段落；新进程显示新增中英文和原正文。独立 ZIP/XML 核对新增段落、原强调文字、尾段及一张两行表格均保留。 |
| XLSX | 将 B2 从 2 改为 4；D2 重算为 60，D5 合计为 322.5。新进程仍显示这些结果；独立 XML 核对四个公式及对应缓存值，未把公式降为数值。 |
| PPTX | 将第一页文本框内容改为 `Release 中文演示 2026.11`；新进程画布与缩略图显示新内容且仍有两页。独立 XML 核对新文字、第二页文字不变。此操作替换了该文本框的全部文字，不宣称保留被主动替换的旧副标题。 |
| PDF | 将已有页面文字改为 `Weave release invoice 2026.11`，在文本编辑状态直接 ⌘S 提交并保存；新进程显示新正文。pypdf 和 Poppler 读取同样的内容，保留一页、两张图片、零批注；Poppler 独立渲染也已目视核对。较长正文在原文本框内换为两行，读取比较仅合并这个换行。 |

四个文件重开后均为干净状态，退出时没有保存提示；重开前后的已保存文件 SHA-256 相同，四个原始样本 SHA-256 也保持不变。

原生退出补验：在两份原有干净文件之外新建自有空白 PDF，点击 AX 提供的原生最小化按钮后直接发送 ⌘Q，未在中间调用可能重新激活窗口的读取方法；随后出现针对该 PDF 的自建保存确认，取消后保留三个标签。这证明该自动化操作序列能进入和取消退出，但未单独录像证明窗口最小化动画。之后打开原生文件面板，通过系统菜单“退出 Weave Office”结束面板并回到同一保存确认；取消后全部标签仍在。最后仅对自有空白 PDF 选择“不保存”，保留原有两个文件。确认框截图为 `release-panel-quit-confirm-20261102.png`。

文件面板快捷键对照：Weave 与 macOS 文本编辑器的原生“前往文件夹”字段中，Right、Shift+Left 能选中路径末尾的 `f`，但同一桌面工具投递的 ⌘A 和粘贴都保持该选择且不改变字段。文本编辑器的两个面板随后取消，未打开或修改其中原有文档。这个独立系统应用对照说明现有自动化结果不足以将问题归因于 Weave，因此没有添加拦截或重放系统按键的补丁；也没有把 ⌘A/⌘V 记作通过。Weave 面板内自动化 ⌘Q 仍未触发退出，但菜单点击路径已通过。

本轮发现并修复系统面板语言不一致：应用中文界面下，Weave 的原生打开/保存面板显示英文，而同机系统文本编辑器显示中文。生成的 Info.plist 只有 `CFBundleDevelopmentRegion=en`，现增加 `CFBundleLocalizations=[en, zh-Hans]`，声明实际支持的两种界面语言；没有修改系统设置或替换原生面板。重新打包后的可执行文件 UUID 不变，严格签名检查通过；全新进程的系统打开面板显示“打开/取消”，另存为面板显示“保存/取消/保存为”，取消后文档保持干净。AX 记录为 `release-native-open-zh-20261102.txt` 与 `release-native-save-zh-20261102.txt`，安装日志为 `release-native-locale-install-20261102.log`。此处实测中文系统环境，未切换操作系统语言验证其他语言。

验收结束已重开原来的表格与 PDF，恢复表格 J9/100%、PDF 编辑与注释面板/62%；原三个受保护文件 SHA-256 不变。

## 2026-11-02 字体弹窗真实预览

原安装版的文字字体弹窗中，选择 Georgia 后字段已更新，但 `AaBbYyZz 中文` 示例仍用界面无衬线字体显示；问题已在自有干净空白文档中复现，取消后直接关闭，没有修改原文件。旧版截图为 `weave-font-dialog-georgia-before-20261102.png`。

Word 字体弹窗现复用文档 snippet 的排版与渲染，按照当前字段构造临时样文，拉丁与亚洲字体分别生效；菜单、按钮的 SF/PingFang 界面字体保持原链。预览固定高 88 pt，大字号按该区域裁切，不向下移动确定/取消按钮；缺字体按原文档字体规则回退，NaN/inf 继续使用安全字号。缓存只保留当前一张预览纹理，按样文全部字符属性、样式、字体 generation、主题、显示比例和尺寸区分。AX 仍保留预览样文，未增加屏幕说明文字或其他控件。

- `wordcraft-ui-egui --lib font_dialog` 的 6 项通过，包含两项新增的实际弹窗纹理验证：内嵌无衬线与衬线字体的像素确实变化，常用效果确实进入渲染，重复帧复用纹理；72 pt 不增加预览区与按钮位置；文档、版本、dirty、撤销、选区和待应用格式保持不变。另一项覆盖缺字体回退、NaN/inf、深色自动文字和 2× backing 分辨率。日志为 `font-dialog-preview-tests-fixed-20261102.log`。首轮两项新测试因测试 helper 未确认剩余纹理 delta 失败，修正测试的 delta 清理后通过，没有削弱原断言。
- 既有 `font_menu_entries_leave_installed_families_unloaded` 1 项通过，字体列表的临时读取策略保持不变；日志为 `font-menu-memory-tests-20261102.log`。预览当前选择的字体仍可由正常文档引擎按需加载，此结果不声称字体数据库永远不增加所选字体。

首次实机预览还暴露了原 gallery raster 的 macOS 额外描粗：字体虽已正确切换，Regular 仍偏重。最终字体弹窗跟随 hosted 正文关闭该额外描粗，缓存按 hosted 状态隔离；standalone 和其他 gallery 保持原渲染。扩展后的 6 项字体弹窗检查再次通过，在本机 Mac 分支核对 Regular 墨量下降和真正 Bold 仍然变化；日志为 `font-dialog-preview-weight-tests-20261102.log`。非 Mac 像素一致性分支在源码中保留，本机未执行跨平台验证。

该轮优化包 UUID `4B60F04D-C1E0-3600-A751-F9B1037FA004` 已安装并启动，构建约 1 分 09 秒，格式和严格签名检查通过。1280×706 逻辑点窗口中，实际选择 Georgia 与“宋体-简”，预览分别显示对应中英文字体；输入 72 磅后样文在固定区域内裁切，确定/取消仍保持原位置且完整可见。切为深色模式重新打开字体窗口，自动文字和按钮均可读；随后取消、恢复浅色，空白草稿仍为 0 字并可直接关闭，不出现保存提示。最后恢复原两份文档的表格 J9/100% 与 PDF 编辑、注释/62%，三个受保护文件 SHA-256 不变。

GUI 截图为 `weave-font-dialog-georgia-final-20261102.png`、`weave-font-dialog-72pt-final-20261102.png` 和 `weave-font-dialog-dark-final-20261102.png`。构建、安装日志分别为 `release-font-preview-weight-build-20261102.log`、`release-font-preview-weight-install-20261102.log`，源码摘要和验收结果记录为 `font-dialog-preview-verified-20261102.json`。

## 2026-11-02 专业弹窗与统一工作区操作边界

实际对照发现：专业弹窗（如表格“设置单元格格式”）打开期间，⌘N/⌘W 不应创建或关闭文件；Weave 原安装包 `4B60F04D-C1E0-3600-A751-F9B1037FA004` 却会在 ⌘W 后关闭整个表格标签。观察使用自建空白文档，原有两份验收文件均保留。

四个 hosted 适配现在提供专业弹窗状态。宿主据此禁用文件标签切换、关闭、新建，以及共享文件菜单与快捷工具；精确消费宿主管理的文件快捷键，复制等文本编辑事件和额外修饰键继续交给编辑器。弹窗自己的输入、绘制、引用选择和已确认的保存/导出请求继续运行。Finder 与文件拖入另行排队，弹窗结束后按顺序打开；它们不会挡住后面的已确认保存请求。应用退出保留统一保存事务，取消后原专业弹窗继续存在。

普通工作窗格不阻塞宿主操作：Word 查找导航窗格与 PDF 合并工作页通过真实命令打开后仍可使用宿主新建快捷键。PDF 点击外部即可关闭的命令搜索浮层也排除在专业模态状态之外；该浮层本轮未做原生 GUI 专项回放。

- 五项 `editor_modal` 回归通过：四类真实专业弹窗打开后的快捷键保护；标签切换、关闭和“+”的实际指针命中；Word/Sheet 取消后恢复；共享工具栏/文件菜单禁用与 Quit 可用；外部打开排队但已确认保存可写出并独立读取；退出取消保留文字与弹窗。其中保存请求由测试通过真实宿主 dispatcher 注入，未据此声称逐个高级编辑器弹窗的请求产出都已覆盖。日志 `editor-modal-contract-tests-toolbar-20261102.log`。
- 暂时移除宿主专业弹窗门禁后，同一个快捷键测试按预期失败；随后恢复源码。负向对照日志 `editor-modal-negative-control-20261102.log`。
- 恢复后的联合回归为 65 项 library、1 项混合视图、2 项逐帧绘制、4 项外部文件互操作，合计 72 项通过；另外新增的 `navigation_work_panes_keep_host_shortcuts_available` 1 项通过。日志 `editor-modal-integrated-tests-20261102.log`、`navigation-pane-contract-tests-20261102.log`。这轮未新增或复制任何字体或图标资产。

原生 GUI 在优化包 `0E704B76-4B62-3066-AA12-FAE3FC1875F1` 中确认表格格式弹窗内 ⌘N/⌘W 均保持原标签，取消后新建和关闭恢复；Word 字体弹窗内输入 18 磅草稿，⌘Q 进入统一保存确认，取消退出后原文字与 18 磅字段继续保留。临时文字文档随后取消字体设置并选择不保存，未对原文件写入。

实机同时发现“设置单元格格式”数值页的两列重叠：弹窗的最小宽度已经是 520 pt，但 `columns` 在测量帧按过窄的可用宽度放置第二列，滚动列表再撑宽第一列。最终改用 152 pt 分类列、16 pt 间距和余下宽度的预览区，子 UI 提供足够的测量高度，两个列表各自最多 216 pt；不强制占满测量高度，不改字号或确认语义。首次列宽修复的 0 高度子 UI 曾使列表只显示少量条目，GUI 发现后已改正。此处属于局部布局调整，以构建、格式检查和实际窗口复验为依据，未重复扩大整组引擎回归。

最终安装包 UUID `52592DE9-2046-37DB-AB29-1D26C9D0AE5F`，优化构建约 50.35 秒，严格签名检查通过。实际窗口中分类与预览分离，选择“货币”会更新格式代码，两边列表均独立滚动到末尾，确定/取消始终完整可见；同包再次验证 ⌘N/⌘W 不穿透专业弹窗，取消后空白表格可直接关闭。最后只保留原两份验收文件，恢复表格 J9/100% 与 PDF 编辑、注释/62%；三个受保护文件 SHA-256 均不变。构建与安装日志为 `release-format-columns-height-{build,install}-20261102.log`，源码摘要与分包 GUI 证据记录在 `editor-modal-contract-verified-20261102.json`。

截图为 `weave-format-columns-final-20261102.png`、`weave-format-columns-both-scroll-final-20261102.png` 和 `weave-modal-word-quit-cancel-20261102.png`。字体、尺寸与工作区边界的这些是代表性验收，不表示所有专业菜单均已逐项验收。

## 2026-11-02 宿主弹窗、批量打开与当前包独立运行

完成审计发现专业弹窗之外的宿主边界缺口：设置、关于、错误和打印已被定义为阻塞窗口，但 Finder/拖入的打开请求仍会执行。新增测试在原实现下确实将活动文件从 Tab 1 切到 Tab 2；原安装包 `52592DE9-2046-37DB-AB29-1D26C9D0AE5F` 也通过 Finder 的一次性“打开方式 → Weave Office”在设置窗口未关闭时切入了自建 DOCX 副本。未更改默认文件关联。

`process_actions` 现让外部打开同时等待宿主和专业编辑器弹窗；已经确认入队的保存、带路径导出仍可执行。普通批量打开遇到错误时暂停，保留后续请求，关闭错误卡片后继续，避免多个坏文件的错误互相覆盖或后续文件提前激活。错误正文补充失败路径，并提供中英文引导文字，保留原解析原因和现有卡片布局。

- 三项新增行为回归覆盖四种宿主弹窗、等待顺序、已确认保存的独立读回，以及有效/坏/坏/有效批次的逐次恢复和原文字保留。原实现的两个失败日志为 `host-modal-external-open-before-20261102.log`、`batch-open-errors-before-20261102.log`。修复后全部 69 项 library 通过，见 `host-modal-batch-library-tests-20261102.log`；增加失败文件路径断言后的同一批处理测试再次通过，见 `batch-open-error-context-tests-20261102.log`，此项不重复计入 69 项数量。
- 优化包 `5888C3B8-3DBC-3CF6-B53B-B26169A4690D` 实机确认：设置保持打开时，Finder 请求不新增或激活标签；关闭设置后同一请求自动打开，无需重发。原生打开面板同时选中 A-first.docx、B-broken.docx、C-last.docx，先显示 A；坏文件卡片出现时 C 尚未打开，关闭卡片后 C 正常进入同一标签栏。自建有效副本随后正常关闭，原两份文件保留。
- 当前安装包 UUID `682F9549-C639-300F-B479-ABED1C1BAC19`，优化构建 21.51 秒，严格签名通过；实机错误卡片已显示 Broken.docx 的完整路径。截图及 AX 为 `host-open-error-context-final-20261102.{png,txt}`。Finder 等待和批处理恢复的截图/AX 分别为 `host-modal-finder-after-waiting-20261102`、`host-modal-finder-after-open-20261102`、`host-batch-error-paused-20261102` 和 `host-batch-after-resume-20261102`。

对当前 `682F9549-C639-300F-B479-ABED1C1BAC19` 再做独立运行：以本地临时目录为工作目录，移除六个字体/翻译构建输入环境变量，Seatbelt 禁止读取整个外置盘、源码、参考仓库和其他办公软件 bundle，并禁止写入 Weave/Craft 偏好目录。对照命令确认源码 README、外置字体清单及其他办公软件 Info.plist 三项读取被拒绝，临时 DOCX 可读。相同应用成功打开五个混合副本，实际检查了中文 Word 标题、正文与两行表格，中文工作簿及计算合计 292.5，两页演示缩略图和中英文字，以及 PDF 已编辑正文、图片与适合整页显示。

正常退出后五份副本的 SHA-256 与应用偏好均不变；随后正常重启并恢复原表格 J9/100% 和 PDF 编辑、注释/62%，三个受保护文件哈希不变，Finder 返回原 acceptance 目录。记录为 `runtime-isolation-current-20261102.json`、`host-modal-batch-verified-20261102.json`，四类截图为 `runtime-current-{word,sheet,slides,pdf-page}-20261102.png`。此验证限定当前 Mac 的进程文件读取隔离，没有据此声称无网络运行、新机器安装或最低系统版本兼容。

原生隐藏后 Dock 退出尚未实机验收：Dock 进程仍存活时，桌面接口按名称和 bundle ID 的读取均超时；后续尝试先发送 Ctrl+F3 聚焦快捷键、再尝试 Up 上下文菜单入口，两次读取也都超时；最后一次在确认 Dock 仍存活后按 bundle ID 复核，仍然超时，累计五次。这里没有观察到足以确认聚焦或菜单已打开的状态，因此不把快捷键调用成功当作完成验证。退出探查后已重新核对原两份文件及 PDF 编辑、注释/62% 的干净状态，文件哈希不变。没有把观察失败当作应用退出故障，也没有因此修改底层退出事件。真实 IME 候选与持续拖窗/拖标签仍按用户“稍后体验”保留。

发布一致性复核未发现安装包 UUID、源码哈希、69 项 library 日志、失败路径专项日志及当前包独立运行记录之间的矛盾。上述未验的原生交互仍保留为未通过，不能据此把整个产品验收表标为全部完成。

## 2026-11-02：字体共享、包体积与合并边界

当前安装包 UUID 为 `1964B381-ECC0-3DF0-B7A0-E83FD1D70A51`。按文件字节合计，`.app` 从 407,542,358 降至 242,432,022 字节，减少 165,110,336 字节（40.5%，十进制约 408 → 242 MB）。原来核查的 58 个字库在二进制中占 239,010,920 字节，含 136,816,852 字节重复副本；新版同一清单逐字节扫描均仅出现一次，共 102,194,068 字节。不是通过修改字形、删语言或依赖外置字库取得此结果。

`weave-font-assets` 按真实内容去重并拥有命名静态数组；Word/Grid/Deck/PDF 的生成字体表及 Word/PDF/宿主的现有拉丁字体借用这些数组。原有清单筛选、脚本覆盖、后备优先级和文档 serif 保持。根 release 配置移除符号，保留默认 unwind；PDF UI 的 WGPU 成为 standalone 默认 feature，Weave 关闭该默认项，只选同一 Glow 后端。

本轮 78 项检查通过：69 library、1 hosted views、2 paint continuity、2 跨编辑器字体指针/覆盖检查、4 外部文件互操作。格式检查与严格签名检查通过。安装包在禁止读取外置盘、源码、参考仓库和其他办公软件的进程中打开五个混合文件，实际核对了 Word 中文/英文/表格、Sheet 中文和合计 292.5、Deck 两页与中英文、PDF 正文 678.90 及彩色图片，并正常退出（返回码 0）。五份副本及宿主自定义设置文件哈希保持不变。原工作区已恢复至两个干净标签：Sheet J9 / 100%，PDF 编辑与注释面板 / 62%；三份受保护原文件哈希保持不变。

首个非保活测试进程在文字截图后退出，未捕获返回码，标准错误为空，未找到匹配崩溃报告；原因未确定。随后保留父会话重跑四类界面、回切及正常退出均通过。此记录不把首轮退出宣称为已修复的应用缺陷。证据在外置 `acceptance/` 的 `bundle-size-before/after-20261102.json`、`shared-fonts-release-verified-20261102.json`、`shared-fonts-runtime-20261102.json` 和对应截图、构建与测试日志。

架构审计同时纠正完成度：已统一的是宿主窗口/标签、设置、文件身份和应用操作。字体字节共享不等于 FontDb/发现/缓存共享；保存路由统一不等于序列化后的原子落盘统一。原编辑器自动恢复被禁用后，宿主自动保存与异常退出恢复尚未接上；重复 craft-libs revision、部分渲染/字体依赖也未收拢。它们已写入 PLAN，不能将“底层全部合并”标为完成。

## 2026-11-02：依赖融合与体积

五个合并到 main 的改动（#2–#6）收拢了四个编辑器的重复依赖，不改变文档引擎：craft-libs 统一到 `c5a1cbf`；宿主构建关闭四个编辑器的 `updater` feature；WordCraft、DeckCraft、hayro-interpret 对齐 egui 的 skrifa 0.44 / harfrust 0.12；hayro 移植到共用的 vello_cpu 0.2 并保留 PdfCraft 的混合与描边修复；宿主只嵌入简体中文界面翻译。

| 状态 | Linux x86_64 release 二进制（字节） |
| --- | --- |
| 融合前 | 273,319,392 |
| craft-libs、更新器、字体栈 | 268,130,128 |
| PDF 渲染改用 vello_cpu 0.2 | 267,530,704 |
| 只嵌入简体中文翻译 | 257,736,016 |

合计减少 15,583,376 字节（5.7%），其中约 102 MB 为字体数据。测量在云端 Linux 上进行，不是 macOS 安装包；`.app` 体积需本机重新打包后记录。宿主回归 82 项（68 library、5 host integration、1 hosted views、2 paint continuity、2 shared fonts、4 互操作）在第 4、第 5 步后各通过一次；pdfcraft-render 483 项、hayro 11 项、vello_common 描边 3 项通过。二进制中核对过 Word/PDF 简体中文词条仍在、其他语言已移除。尚未做 macOS GUI 复验。

## 继续验收的重点

1. PDF 复杂正文与字体、图片自由缩放/裁剪等更多原生回放；已有正文、图片及页面旋转/插入/删除撤销样本不代表全部 PDF 兼容。
2. 更大批量及更多格式的打开、原生窗口中的标签溢出定位与公共设置回放；有效/坏/有效的批次恢复及设置期间 Finder 等待已经通过。
3. 更新后的标题栏可拖动区域、标签拖动排序、原生系统窗控命中、操作系统隐藏后的 Dock 退出，以及系统文件面板内的键盘快捷操作；最小化后的 ⌘Q 恢复及取消已通过。
4. 系统输入法组合输入及更多实际文档的视图状态回放；四类合成文件的选区、滚动和缩放已验证，跨类型富文本、图片等粘贴仍需扩大验证。本轮 GUI 语言切换及演示提纲导出已通过，其余高级格式仍需分别验收。
5. 代表性复杂文件、真实打印机、其余三种编辑器的高级导出/模板接口；干净环境重建及发行兼容性。PDF 的生成文档和高级写出已接到宿主契约，尚需扩大原生界面回放。
6. 对话框尚未覆盖的 vendor 专业弹窗及 PDF prompt 交互需按需继续验收；当前不把所有专业属性表单记为已 GUI 验收。IME 和持续的原生拖窗/拖标签交互待用户稍后体验。

验收范围和实施目标见 [规划文档](../PLAN.md)，PDF 的具体边界见 [PDF 能力记录](pdf-capabilities.md)。

## 2026-11-02：代码层融合（二）

用户要求把剩下的代码层融合一次做完、合并后统一验收。合并到 main 的改动如下：

| PR | 内容 |
| --- | --- |
| #8 | 新增 `crates/system-fonts`：系统字体只扫描一次，文字、表格、演示的字体列表与查找共用 |
| #9 | 宿主自动保存与崩溃恢复（`src/recovery.rs`），下次启动提示恢复 |
| #10 | 表格的 PDF 导出与打印改用共享的 craft-pdf，中文字体嵌入（原写出器不嵌入 CJK 字体） |
| #11 | 文字的 EMF/WMF 改用演示的 metafile 解析器；测试逐例断言与原解析器结果一致 |
| #12 | 四个编辑器共用一套 egui 字体（`src/font_set.rs`），切换标签不再重装字体和字形图集 |
| #13 | 公共组件 `crates/ribbon`（`weave-ribbon`），功能区标签行由宿主统一绘制，编辑器只画当前标签的命令区 |

宿主回归 98 项全部通过：78 library、4 外部互操作、5 host integration、2 hosted views、2 paint continuity、3 shared fonts、4 weave-ribbon。新增的检查包括：四类编辑器各开两个来回切换后，字体覆盖每个标签页且再切换时不变；四类编辑器的当前标签在无障碍树里只出现一次，点击能切换标签；表格 PDF 用共享引擎写出中文。

Linux x86_64 release 二进制为 258,079,952 字节，比上一节的 257,736,016 字节多 343,936 字节（+0.13%）。去掉的重复代码（表格自带的 PDF 写出器路径、文字的 metafile 解析器）本来就小，新增的崩溃恢复、共享字体扫描和公共功能区组件抵消了这部分，所以这一轮的价值是去掉重复实现、统一行为，不是缩小体积。

需要在 Mac 上验收的观感变化：

- 四个编辑器的功能区标签行样式现在完全一致（宿主面板底色、13 pt、强调色下划线）。文字标签放不下时改为收进 » 菜单，不再省略文字。
- 文字标签上的右键“自定义功能区”在宿主中不再提供。
- 界面字体都没有的字形（少数符号）会走回退字体，回退顺序可能与单独运行时不同。
- 表格导出的 PDF 改用 Liberation / Noto Sans CJK 字体嵌入，拉丁文字的位置与原来一致。
- 文字中的 EMF/WMF 图片改由演示的解析器读取；测试用例上两者结果完全相同，但仓库里没有真实的 EMF/WMF 样例文件。

尚未做 macOS GUI 复验，`.app` 体积也需本机重新打包后记录。
