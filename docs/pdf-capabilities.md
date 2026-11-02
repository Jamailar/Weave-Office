# PDF 编辑能力与验收边界

更新：2026-11-01。证据分为源码已接入、真实引擎测试、原生界面实测；三者不能互相替代。

PDF 已嵌入统一文件标签，并使用 PdfCraft 原生文档、编辑事务、撤销栈和 PDF 写入器。新增验收不再只检查 `page.rotate`：在自行构造、含正文的 PDF 上添加实际 FreeText 批注，保存后以新宿主重开，核对批注内容及原正文。

## 已接入的操作入口

| 能力 | hosted UI 入口与执行路径 | 验收状态与限制 |
| --- | --- | --- |
| 阅读 | `hosted_ui` 的 Read 按钮、`canvas::document_area`、右侧导航栏 | 原生窗口已显示含正文和图片的样本，关闭重开后显示新内容；长文档、复杂选区与字体外观仍需扩充样本。 |
| 已有正文与图片编辑 | Edit 按钮进入 `Mode::Edit`，自动启用 `QuickTool::EditText`；复用 `panels/edit.rs` 和画布对象编辑 | 原生窗口已替换已有英文正文；通过系统文件面板替换原图，旋转 90°、移动后另存并重开。独立解析与渲染确认正文、图像像素及位置矩阵，且不是批注；自由缩放、裁剪、复杂字体及混排仍需代表性样本。 |
| 添加正文、图片及页面装饰 | Edit 面板的 Add content 与 More；命令走 `apply_edit`，图片走既有文件选择器 | 原生窗口添加自建 PNG 并保存重开通过：原图片不变，新增图片具有不同内容。正文添加、页眉水印等尚未全部回放。FreeText 测试仍仅代表批注。 |
| 批注 | Add comments 按钮进入 comment 工具，打开 Comments 侧栏；便笺、文本框、标记、线条/形状及回复由现有面板处理 | FreeText 的编辑事务、撤销/重做与保存重开有针对性测试。未 Post 的便笺、回复、新评论和批注编辑草稿计入 dirty；允许切换标签，返回后草稿仍在。保存明确要求先 Post 或 Cancel；关闭交由宿主提供保存、不保存、取消选择。 |
| 填写文字 | 现有 Fill & Sign 工具中的 TypeBox；`fill_sign::typed` 创建 Typewriter/FreeText 批注 | 测试直接使用同一编辑动作及实际 TypeBox 草稿，检查保存先提交草稿。没有新增依赖或系统字体读取。顶层只保留 Read/Edit/批注/整理页面，不声称已有独立 Fill & Sign 顶层按钮。 |
| 页面操作 | Organize pages 按钮；整理面板和页面网格支持插入、删除、重排、旋转、框裁切等 | 原生窗口完成页面旋转、从另一 PDF 插入、删除后统一撤销，再另存并重开两页文档；独立解析/渲染及新宿主读取确认两页旋转、正文和图片。提取生成新宿主标签，只有宿主接受且源仍安全时才删除指定源页；拆分与分别提取由原生目录面板及宿主写入保护承接。 |
| 保存、撤销与重做 | 宿主统一 Save/Save As、Undo/Redo；PDF 保留 `edit.undo`/`edit.redo` 命令及快捷键，hosted 工具栏不重复添加按钮；`hosted_save` 先 flush，再序列化、校验、原子写入、mark_saved | 宿主与 PDF 共用实际编辑状态。新增测试检查正文/批注内容，不能仅以 dirty 消失或文件非空判定成功。 |

入口证据：`src/editor.rs`，`vendor/pdfcraft/crates/ui-egui/src/app/hosted.rs`、`panels/edit.rs`、`panels/organize_panel.rs`、`tools/comments/`、`tools/fill_sign.rs`、`editing.rs`。

### 高级工具的文件选择

PdfCraft 将打开单文件/多文件、保存目标和文件夹选择整理为带标题、初始目录、文件名及扩展名过滤器的 `HostFileRequest` 描述器。在 hosted 模式，`take_host_file_requests` 将请求交给 Weave Office，由统一宿主管理队列并调用操作系统原生文件弹窗，再通过 `hosted_file_result` 把选择路径或取消结果送回原 PDF 编辑器。PdfCraft 保留原操作回调，等待中的第二个选择请求会在第一个完成后继续；独立运行模式也通过 `into_native` 调用 `rfd::AsyncFileDialog`。已在 hosted 图像工具与拆分工具中实际确认系统文件/文件夹面板和取消路径，其他高级流程仍需逐项回放。相关契约源码为 `vendor/pdfcraft/crates/ui-egui/src/platform/file_dialog.rs`、`platform/pickers.rs` 和 `src/app.rs`。

选择取消不执行原操作。返回结果前会检查请求 ID、单选数量、源文档是否仍存在，以及需要保护的编辑目标是否已变化；不符合条件时拒绝执行。回调是 `FnOnce`：请求校验失败及回调异常可返回宿主；导出写入器的普通失败由工具自身提示，不能据 `hosted_file_result` 返回成功认定文件已写入。原请求已经消耗，须重新发起该工具操作。`tests/pdf_host_file_dialogs.rs` 覆盖描述器往返、取消、连续选择、源关闭和目标变化等路径；它们是无窗口的契约测试，与上述 GUI 面板验收分别记录。

打开、保存、另存为、文件夹及其他文件路径选择（含原生面板中的覆盖确认）使用操作系统原生界面；应用菜单、设置、内部关闭确认、错误提示和打印界面由 Weave Office 自建。PDF 工具交宿主的文件请求遵循同一边界。

## 可重复的真实引擎测试

执行命令（根目录；构建缓存盘需已挂载）：

```sh
PDFCRAFT_SYSTEM_FONTS=0 python3 scripts/cargo.py test --test pdf_editing --locked --offline -j 3
```

测试位于 `tests/pdf_editing.rs`，在临时目录现场生成带完整 xref 的一页 PDF，原正文为 `Weave source invoice 123.45`。源 PDF 仅使用标准 14 字体名称 Helvetica 和内建度量；ASCII 用例不需要外部字体。中文用例严格要求构建时的开放 craft-fonts 输入，不读取系统字体，且无字体时明确失败而非忽略。生成的中文批注实际嵌入开放字体程序；测试不使用或修改用户文档。

| 测试 | 内容证据 |
| --- | --- |
| `pdf_text_annotation_undo_redo_save_and_reopen_preserve_source_content` | 添加 `Reviewed amount: 123.45`；断言 FreeText 类型、正文、作者、有效几何框、零旋转及原页面文本。Undo 后批注消失且恢复干净状态，Redo 后批注恢复；另存后原文件字节不变，销毁旧宿主并从磁盘新建宿主重开，重新检查内容。 |
| `chinese_pdf_annotation_persists_real_glyphs_without_system_fonts` | 写入“金额已核对，共计123.45元”，覆盖撤销/重做、保存、新宿主重开、原正文与原文件不变、无缺字提示。使用真实 PDF 引擎离屏渲染保存的批注，检查有字形像素且不同于逐字替换为问号的对照。 |
| `chinese_pdf_tab_keeps_its_contents_and_renders_like_a_space_after_reopen` | 新宿主重开后 TAB 内容原样保留，无缺字提示；保存的中文 TAB 外观与显式空格外观的实际离屏像素完全相同。 |
| `pdf_save_commits_an_unfinished_typewriter_draft_before_writing` | 将实际页面 TypeBox 保持为未提交状态，保存必须提交并清除草稿；重新打开后批注仍存在、原正文不变。 |
| `unfinished_comments_survive_tab_switches_and_block_saving_without_losing_the_draft` | 覆盖便笺 composer、reply、add_box、编辑为空文本及 composer 编辑为空文本。检查 dirty、关闭保护、明确保存错误、切换到另一宿主标签并返回、文件未改写及评论和并存的 TypeBox 输入原样保留。中文在此只证明草稿数据保留，不证明 PDF 中文字形绘制。 |
| `hosted_pdf_cannot_hide_another_document_inside_its_host_tab` | 对已有批注的 PDF 触发字节打开、历史修订、签名版本、评论摘要、Combine 及提取；独立结果排队交宿主，源实例始终只留一个 view 和一个引擎 document。宿主取消结果时源内容不变；路径打开和全屏请求交回宿主。 |

中文 FreeText 分支复用 `pdfcraft_fonts::source_shaping` 的整形和 `embedding::embed_mapping` 的 CID/ToUnicode/字体写入器，不另造字体系统。ASCII 保持原 Helvetica 路径；Unicode 的分行、创建框尺寸、改文自动高度与绘制共用所选字体的实际整形度量（含数字和空格），TAB 仅在外观中归一为空格，原始 Contents 保留。窄框回归从保存后的 ToUnicode 解码实际绘制内容，检查尾字符、分行宽度与高度。Unicode 选择构建内的开放 Hans/Jpan 字体，嵌入完整程序并在同一文档复用。此方案暂不裁剪字形子集，首次中文批注会增加字体程序大小。注释模块另有 `chinese_freetext_embeds_unicode_font_and_remains_editable_after_reopening`，检查保存后的 Type0 字体、ToUnicode、实际嵌入轮廓、重开继续改文/改色和字体程序复用。

上述 6 项宿主 PDF 测试与批注模块的 77 项单元测试已通过；另有 7 项生成文档和 4 项高级导出保护测试通过。原生窗口已完成便笺、已有英文正文替换、新增图片、原图替换/移动/直角旋转的保存重开，以及混合标签切换。原图样本另经 pypdf 核对实际绘制、矩阵及像素，并经 Poppler 独立渲染；中文 FreeText 的字形验证使用真实 PDF 引擎离屏渲染。复杂字体和混排文件尚未完成外观对照，因此不将此结果写成“PDF 全功能完成”。具体命令与界面证据见 [本机验收记录](acceptance.md)。

图片右键菜单独占其页面弹出层，避免通用文字菜单在同帧覆盖图片按钮的标识与操作。两项 hosted 界面回放覆盖阅读选区后进入编辑、图片菜单旋转命令及普通文字/空白菜单；临时恢复双菜单绘制会让该回归失败。原生窗口中已复现修复前的菜单重叠，并确认修复后只显示对应菜单。

## 宿主边界与剩余工作

停用 PDF 编辑器会释放指针手势、渲染需求和 IME/焦点，但不要求先 Post 评论。有未 Post 评论时，同时保留该编辑器的其他输入草稿，防止自动提交 TypeBox 所触发的上游批注回调清除空文本编辑；没有评论草稿时继续提交页面、表单及 TypeBox 输入。保存仍通过严格 flush，未 Post 评论不会被误标为已保存。

普通打开、保存、关闭、退出、偏好、主题和全屏注册命令通过 `take_host_commands` 回到 Weave Office。独立路径打开也交回 `file.open {path}`；直接全屏方法交回 `view.full_screen {enabled}`，不向共享窗口直接发送全屏命令。宿主必须实际消费这些参数，不能把带路径的请求都变成打开对话框，也不能忽略指定全屏状态。

所有已有内部 `views.push` 路径已检查。`HostedDocument` 传递独立结果的名称、字节和必要的内存密码，`take_host_documents` 由宿主消费，`hosted_open_bytes_with_password` 创建新的单文档编辑器。转换、批注摘要、签名版本、历史修订及合并结果均走该契约。提取删除需 `hosted_document_result` 成功确认，并重新核对源身份、编辑代数和草稿。Combine 使用源标签内的临时工作视图，关闭后返回源，不新增隐藏文档。签名结果也作为新文件交宿主，保留源路径和保存点。

`HostExportWriter` 将高级导出、拆分、分别提取、附件、报告及后台 export/OCR/action 的写入交宿主。宿主在同一把文件身份锁内检查当前路径并原子写入，打开、另存为和关闭同步更新保护集合；后台任务克隆实时服务，不保存过时路径快照。可信 `hosted_save` 仍由宿主保存事务管理，允许保存自身文件；导出副本拒绝所有已打开路径。批量写入失败不执行删除源页回调。

`hosted_poll_background` 将 OCR、优化、导出、Action 和打印结果接收从活动视图逻辑中拆出，宿主对所有标签调用；它不处理输入、安装字体或修改主题。任务、待保存结果和生成文档确认仍未完成时，宿主保留标签并阻止关闭事务。普通渲染及字体发现不阻塞关闭。Read 模式中等待应用的隐式 OCR 在关闭准备时结束，已识别页保留，继续进入正常保存或不保存判断；主动发起的任务仍需完成。文件选择回调留在来源编辑器上下文中执行。

在原生 hosted 模式，图片/字体/附件导入及高级工具的文件路径请求走上述宿主描述器，并由宿主呈现操作系统原生文件弹窗；工具自身的专用面板、确认对话框和完整操作流程仍需逐项原生回放。高级工具可经 Edit 的 Other options、整理页返回工具列表和右侧面板进入。本文未宣称 OCR、表单、数字签名、加密、复杂正文重写或任意字体/复杂混排的中文 PDF 绘制通过完整产品验收。
