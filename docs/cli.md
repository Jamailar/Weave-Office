# 命令行(CLI)

Weave Office 的可执行文件本身就是命令行工具:第一个参数是下面的动词时,不开窗口,直接在四个编辑器的引擎上创建、读取、修改文档。设计目标是让 AI 代理和脚本能可靠地操作文档;思路参考了 [OfficeCLI](https://github.com/iOfficeAI/OfficeCLI)(只借鉴设计,不含其代码)。

给 AI 读的完整说明随程序发布,运行 `weave-office help agent` 即可打印(源文件 [crates/cli/AGENT.md](../crates/cli/AGENT.md))。

## 命令

| 命令 | 作用 |
|---|---|
| `create <文件> [--template 名] [--force]` | 按扩展名新建空白文档 |
| `info <文件>` | 类型、页数/工作表/幻灯片、元数据 |
| `view <文件> [outline\|text\|json]` | 带路径的大纲(默认)、纯文本、完整结构 |
| `get <文件> <路径>` | 读一个节点 |
| `set <文件> <路径> key=value…` | 改节点属性:文字、样式、加粗、颜色、单元格值/格式、版式、旋转等 |
| `add <文件> <父路径> --type 类型 [key=value…] [--after/--before 路径 \| --index N]` | 新增段落/标题/表格/分页符、工作表、幻灯片/文本框、PDF 页/文字 |
| `remove <文件> <路径>` | 删除段落/表格、工作表/清空单元格、幻灯片/形状、PDF 页 |
| `commands <类型\|文件> [关键字\|命令id]` | 列出引擎命令和参数 |
| `run <文件> <命令id> [key=value…] [--params JSON]` | 执行一条引擎命令,有改动才保存 |
| `batch <文件> [--input f.json \| --commands JSON]` | 执行一串命令(默认读 stdin),全部成功才写盘 |
| `export <文件> <输出>` | 另存为 PDF、PNG、CSV、HTML、Markdown、TXT、DOCX 等 |
| `help [agent\|paths\|props\|命令]` | 帮助;`props` 列出 set/add 支持的属性 |

通用选项:`--json`(统一 JSON 信封)、`--out 文件`(写到另一个文件)、`--dry-run`(不写盘)、`--keep-going`、`--save`、`--password`、`--page N`。

类型名:`word`(.docx .odt .rtf .md .txt .html)、`sheet`(.xlsx .ods .csv)、`slides`(.pptx .odp)、`pdf`;也接受 `docx`、`excel`、`ppt` 等别名。

## 输出与错误

`--json` 时成功为 `{"ok":true,"data":…}`,失败为 `{"ok":false,"error":{"code","message","suggestion","candidates","details"}}`;退出码 0 成功、1 失败、2 用法错误。错误码:`usage`、`not_found`、`exists`、`unsupported_format`、`unknown_command`(`candidates` 给出最相近的命令 id)、`invalid_params`、`invalid_path`、`password_required`、`open_failed`、`batch_failed`(`details.results` 列出每一步的状态)、`io`、`engine`。

## 路径

1 起算,`/` 是整个文档。文字:`/body/p[N]`、`/body/table[N]`、`/body/block[N]`;表格:`/Sheet1`、`/Sheet1/B2`、`/Sheet1/A1:C10`、`/sheet[N]`;演示:`/slide[N]`、`/slide[N]/shape[M]`、`shape[@id=…]`、`shape[@name=…]`;PDF:`/page[N]`。`view <文件> outline` 会列出所有路径。

`set`/`add`/`remove` 是对引擎命令的组合(定位光标、选中、格式化),效果与在编辑器里同样操作一致;回复里带 `commands` 字段列出实际执行的引擎命令,`set`/`add` 还返回改动后的节点。batch 里也能混用:`{"set":路径,"props":{…}}`、`{"add":父路径,"type":…,"props":{…},"after":…}`、`{"remove":路径}`。

注意:引擎命令的参数沿用各引擎自己的约定,例如演示命令里的 `index`/`at` 从 0 开始,PDF 工具的 `page` 从 1 开始。

## 写盘

每次调用都是全新的无界面会话:打开文件、执行、成功后保存。保存先写到同目录的临时文件(同扩展名),再改名覆盖,所以文件要么是完整新版本,要么保持原样。只读命令不会重写文件。光标/选区不跨调用保留:优先用按路径的 `set`/`add`/`remove`;直接用引擎命令且步骤有前后依赖时,放进同一个 `batch`。

## 实现

代码在 [crates/cli](../crates/cli)(`weave-cli`),只依赖四个引擎(`wordcraft-engine`、`gridcraft-engine`、`deckcraft-engine`、`pdfcraft-automation`),不依赖界面。`src/main.rs` 在第一个参数是动词且不是现有文件时转交给它,所以不增加第二个可执行文件。开发时可以不构建整个 App:`cargo run -p weave-cli --example weave -- help`。测试:`cargo test -p weave-cli`。

后续:驱动正在运行的 App(让 AI 的修改直接出现在打开的窗口里),以及表格单元格内段落等更深路径的写操作。
