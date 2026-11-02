# Weave Office

产品目标、统一架构边界、实施顺序与验收标准见 [产品目标与实施规划](PLAN.md)。

**后续 AI 接手入口：[Craft 参考项目、固定版本与接手说明](vendor/README.md#给后续-ai-的参考项目与接手说明)。** 文字基于 [WordCraft](https://github.com/storytold/wordcraft)，表格基于 [GridCraft](https://github.com/storytold/gridcraft)，演示基于 [DeckCraft](https://github.com/storytold/deckcraft)，PDF 基于 [PdfCraft](https://github.com/storytold/pdfcraft)。

目标是在一个桌面应用中，通过统一的文件标签管理文字、表格、演示和 PDF，交互参考主流办公套件的多文件标签体验。项目仍处于集成开发中，**尚不是可日常使用的四合一办公套件**。新版 UI 字体、紧凑尺寸、顶部新建浮层和公共语言偏好已打包并完成指定 GUI 复验。外部来源 DOCX 样张的宋体替代、网格度量、表格边框位置与 macOS 100% 显示已修复并独立对照；替代字形和复杂格式保真仍有限制，输入法和原生拖动待持续体验。

2026-11-02 的体积与架构审计确认：宿主已经统一窗口、标签和应用操作，但公共基础设施尚未全部收拢。字体字节现已单份共享，安装包从 407.5 MB 缩减到 242.4 MB（十进制，减少 40.5%）；核查的 58 个字库均只出现一次。78 项相关检查及四类文件的独立运行复验通过。随后的依赖融合把 craft-libs 统一到一个 revision，四个编辑器共用同一套字体解析/整形库和同一个 CPU 光栅化器（vello_cpu 0.2），宿主构建不再带四份上游自动更新器，也只嵌入简体中文界面翻译；Linux release 二进制因此从 273.3 MB 降到 257.7 MB（−5.7%），macOS 安装包待重新打包测量。之后的代码层融合（#8–#13）让系统字体只扫描一次，接上宿主自动保存与崩溃恢复；表格 PDF 导出改用共享引擎，文字改用演示的 EMF/WMF 解析器；四个编辑器共用一套界面字体，功能区标签行由宿主统一绘制。Linux 二进制为 258.1 MB。仍待统一的是序列化后的原子落盘。详见 [规划](PLAN.md) 与 [验收记录](docs/acceptance.md)。

除四种办公文档外，宿主还内置一个轻量文本编辑器（[crates/text-editor](crates/text-editor/src/lib.rs)，只依赖 egui，不引入新的第三方库）：Markdown 采用接近 Obsidian 的实时预览（格式就地呈现，光标所在行才显示 `#`、`**`、`[]()` 等标记，另有源码和阅读视图、大纲、任务复选框、`[[双链]]`），纯文本/代码文件有 20 多种语言的语法高亮、行号、查找替换、转到行、行操作和 JSON 格式化。文件按原编码（含 GBK、UTF-16、BOM）和原换行符写回，未改动时保存不改变任何字节；扩展名不认识但内容是文本的文件也按纯文本打开。导出 PDF 和打印借用文字引擎的 Markdown/TXT 导入。

- [统一工作区状态](crates/workspace/src/lib.rs)：文件类型、标签、未保存状态及保存完成顺序。
- [统一引擎宿主](src/host.rs)与[编辑器适配](src/editor.rs)：管理四类文档的新建、打开、保存与关闭，并使用各上游的 hosted 编辑界面。
- [统一应用外壳](src/app.rs)：统一文件标签、通用命令、最近文件和关闭协调，已实测五个混合文件标签和四类型编辑保存流程。
- [PDF 引擎适配](crates/pdf-backend/src/lib.rs)：底层编辑与保存快照验证；图形界面通过 PdfCraft 的 hosted 接口接入。
- [早期三编辑器实验](integration/README.md)：保留作集成参考，不是正式产品入口。
- [macOS 打包说明](packaging/README.md)：优化 release `.app` 已安装，四格式保存重开及中文系统文件面板已复验；正式发行验证仍待完成。
- [本机验收记录](docs/acceptance.md)：自动测试、GUI 实际操作、范围及剩余工作。
- [命令行](docs/cli.md)：`weave-office create/view/get/run/batch/export …` 不开窗口操作四类文档，面向 AI 代理和脚本；`weave-office help agent` 打印给 AI 的使用说明。

界面语言只控制公共 UI。Word Language Preferences pane 保留并只读显示校对状态；切换语言不会更改文档校对语言、dirty 状态或 undo 历史。第三方办公软件生成的 DOCX/XLSX/PPTX 小样本互操作、PDF 字体优先级修复及原生 AppMenu/快捷键回归见验收记录；这些样本不代表复杂文件的完整格式保真。

专业弹窗与公共文件操作共用门禁，避免在确认或取消前误切换、误关标签；弹窗自己的输入和已确认保存继续工作，外部打开文件会排队。退出仍走统一保存流程，普通查找窗格和 PDF 合并页面继续支持标签操作。

设置、关于、错误和打印窗口打开时，Finder 或拖入的文件也会等待。批量打开中的坏文件会显示具体路径，关闭错误提示后继续后续文件，保留已打开的文件。最新安装包已重新通过禁止读取源码、参考仓库、其他办公软件和整个外置盘的五文件独立运行检查。

顶部“+”下方的新建浮层使用原创图标，按文字、演示、表格、PDF 排列；已通过分帧指针回放与四类新建的实际 GUI 操作。UI 使用 macOS 界面字体及内嵌后备字体，统一常用控件尺寸并压缩功能区留白。PDF 页面内容采用独立字体族，表单、填充签名与逐帧验证通过。安装包默认位于用户 `~/Applications/Weave Office.app`，完整范围及剩余问题见[本机验收记录](docs/acceptance.md)。

界面边界：保留真实 macOS 红黄绿系统窗控及原生圆角、阴影；Weave 自建窗口内容、文件标签和工具栏。顶部标签行可混排四种文件；下一行整合 File、打开/保存/打印/撤销/重做、专业导航标签与统一矢量图标。打开、保存、另存为、文件夹及其他文件路径选择使用操作系统原生弹窗，由统一宿主调度，包括三种 Office 编辑器的图片选择与 PDF 高级工具的选择请求；原生面板中的覆盖确认也使用系统界面。其他应用弹窗由 Weave 自建。已实测系统文件选择及新式保存确认、文件载入错误和打印窗口；各类专业编辑器弹窗的 GUI 覆盖范围见验收记录。

macOS 应用图标使用用户选定的 imagegen D 版源稿，保存在 [应用图标资源](assets/app-icon/README.md)；Word、Grid、Deck、PDF 文档图标继续使用项目原创矢量绘制。`scripts/generate_macos_icons.py` 生成各尺寸并纳入应用包，生成缓存位于外置盘；详情见[打包说明](packaging/README.md)。

本机开发入口：

```sh
python3 scripts/prepare-fonts.py
python3 scripts/prepare-document-fonts.py
python3 scripts/prepare-i18n.py
python3 scripts/cargo.py run --locked --offline
```

以上入口要求先设置环境变量 `WEAVE_DEV_VOLUME`（外置盘挂载点，如 `/Volumes/MyDrive`）并挂载该盘，并统一检查外置构建输出和开放字体输入。生成应用包见打包说明。

正式工程使用 Rust 1.95.0，四个上游的固定版本位于 `vendor/`，来源和许可证见[上游记录](vendor/README.md)。开发机上相邻的本地参考仓库目录仅供查阅，正式入口不依赖它。

## 本机构建缓存

本机 Cargo 构建输出统一写入 `$WEAVE_DEV_VOLUME/DevCache/WeaveOffice/target`，由未提交的 `.cargo/config.toml` 配置。原有 `target` 目录已迁到同一外置盘并保留符号链接；构建时需挂载该外置盘。
