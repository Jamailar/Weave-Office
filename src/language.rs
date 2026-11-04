//! Host-owned interface language. Editors receive a projection, never a preference of their own.
use serde::{Deserialize, Serialize};
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiLanguage {
    #[serde(rename = "en")]
    En,
    #[default]
    #[serde(rename = "zh-hans", other)]
    ZhHans,
}
impl UiLanguage {
    pub const ALL: [Self; 2] = [Self::ZhHans, Self::En];
    pub fn code(self) -> &'static str {
        match self {
            Self::ZhHans => "zh-hans",
            Self::En => "en",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::ZhHans => "简体中文",
            Self::En => "English",
        }
    }
    /// Only aliases for the shared languages; do not silently map Traditional Chinese to Simplified.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "zh" | "zh-hans" | "zh-cn" | "zh-sg" => Some(Self::ZhHans),
            "en" | "en-us" | "en-gb" => Some(Self::En),
            _ => None,
        }
    }
}
// A render-thread projection for callbacks that do not receive the app (toolbars/native menus).
// Only the shared shell publishes it; it is never persisted or restored with a document's Memory.
thread_local! { static CURRENT: Cell<UiLanguage> = const { Cell::new(UiLanguage::ZhHans) }; }
pub fn set_current(language: UiLanguage) {
    CURRENT.set(language);
}
pub fn current() -> UiLanguage {
    CURRENT.get()
}
pub fn tr(text: &str) -> &str {
    if current() == UiLanguage::ZhHans {
        return text;
    }
    match text {
        "文件" => "File",
        "新建" => "New",
        "打开" => "Open",
        "保存" => "Save",
        "另存为" => "Save As",
        "导出 PDF" => "Export PDF",
        "打印" => "Print",
        "关闭" => "Close",
        "设置" => "Settings",
        "关于" => "About",
        "退出" => "Quit",
        "打开 (⌘O)" => "Open (⌘O)",
        "保存 (⌘S)" => "Save (⌘S)",
        "打印 (⌘P)" => "Print (⌘P)",
        "撤销 (⌘Z)" => "Undo (⌘Z)",
        "重做 (⇧⌘Z)" => "Redo (⇧⌘Z)",
        "退出 Weave Office" => "Quit Weave Office",
        "剪切" => "Cut",
        "复制" => "Copy",
        "粘贴" => "Paste",
        "全选" => "Select All",
        "编辑" => "Edit",
        "最小化窗口" => "Minimize window",
        "还原窗口" => "Restore window",
        "最大化窗口" => "Maximize window",
        "关闭窗口" => "Close window",
        "导出 PDF 副本" => "Export PDF Copy",
        "选择文件" => "Choose File",
        "保存文件" => "Save File",
        "选择文件夹" => "Choose Folder",
        "打开文件" => "Open File",
        "无法打开文件" => "Could not open file",
        "办公文件" => "Office files",
        "关闭其他标签" => "Close Other Tabs",
        "关闭标签" => "Close Tab",
        "新建文件" => "New File",
        "最近文件" => "Recent Files",
        "打开或新建文件，开始办公" => "Open or create a file to get started",
        "外观" => "Appearance",
        "浅色" => "Light",
        "深色" => "Dark",
        "跟随系统" => "System",
        "界面缩放" => "Interface scale",
        "关于 Weave Office" => "About Weave Office",
        "文字 · 表格 · 演示 · PDF" => "Documents · Spreadsheets · Presentations · PDF",
        "编辑引擎基于 WordCraft、GridCraft、DeckCraft 和 PdfCraft。" => {
            "Powered by WordCraft, GridCraft, DeckCraft and PdfCraft."
        }
        "操作未完成" => "Action could not be completed",
        "确定" => "OK",
        "保存更改？" => "Save changes?",
        "恢复未保存的文档？" => "Recover unsaved documents?",
        "Weave Office 上次没有正常退出。以下文档有自动保存的副本：" => {
            "Weave Office did not quit normally last time. These documents have automatically saved copies:"
        }
        "恢复" => "Recover",
        "不恢复" => "Don’t Recover",
        "无法恢复 {title}：{error}" => "Could not recover {title}: {error}",
        "不保存" => "Don’t Save",
        "取消" => "Cancel",
        "文字文档" => "Document",
        "文字" => "Document",
        "Office 文档" => "Office documents",
        "电子表格" => "Spreadsheet",
        "演示文稿" => "Presentation",
        "文档" => "Document",
        "表格" => "Spreadsheet",
        "演示" => "Presentation",
        "正在读取打印机…" => "Finding printers…",
        "暂无可用打印机" => "No printers available",
        "份数" => "Copies",
        "双面打印（长边翻转）" => "Print on both sides (long edge)",
        "正在提交打印任务…" => "Submitting print job…",
        "已提交到打印队列" => "Sent to print queue",
        "常规" => "General",
        "语言" => "Language",
        "界面语言" => "Interface language",
        "应用于所有文档窗口，不改变文档校对语言。" => {
            "Applies to all document windows. Document proofing languages stay unchanged."
        }
        "校对语言" => "Proofing language",
        "无法读取系统打印机" => "Could not read system printers",
        "打印任务未返回结果" => "The print job did not return a result",
        "“{title}”有未保存的修改。" => "“{title}” has unsaved changes.",
        "关闭 {title}" => "Close {title}",
        "{kind}图标" => "{kind} icon",
        "未命名{kind}{number}" => "Untitled {kind} {number}",
        "再次选择…" => "Choose Again…",
        "选择导出文件夹" => "Choose Export Folder",
        "导出演示" => "Export Presentation",
        "选择图片" => "Choose Image",
        "图片" => "Images",
        "{extension} 文件" => "{extension} files",
        "{stem}-导出.pdf" => "{stem}-export.pdf",
        "笔记" => "Note",
        "文本" => "Text",
        "文本与网页" => "Text & web",
        "代码与数据" => "Code & data",
        "纯文本" => "Plain Text",
        "富文本" => "Rich Text",
        "新版本 {version}" => "Update {version}",
        "正在下载更新 {percent}%" => "Downloading update {percent}%",
        "重启以完成更新" => "Restart to Update",
        "检查更新" => "Check for Updates",
        "检查更新…" => "Check for Updates…",
        "正在检查更新…" => "Checking for updates…",
        "已是最新版本" => "You’re up to date",
        "Weave Office {version} 是目前的最新版本。" => {
            "Weave Office {version} is the latest version."
        }
        "发现新版本" => "A new version is available",
        "Weave Office {version} 已发布，当前版本是 {current}。" => {
            "Weave Office {version} is available. You have {current}."
        }
        "这个版本需要从下载页手动安装。" => {
            "This version needs to be installed from the download page."
        }
        "更新" => "Update",
        "打开下载页" => "Open Download Page",
        "稍后" => "Later",
        "正在下载更新" => "Downloading update",
        "可以继续工作，下载完成后会提醒你重启。" => {
            "You can keep working. You’ll be asked to restart when the download finishes."
        }
        "取消下载" => "Cancel Download",
        "后台下载" => "Download in Background",
        "更新已就绪" => "Update ready",
        "重启 Weave Office 即可更新到 {version}。有未保存的文档时，会先询问是否保存。" => {
            "Restart Weave Office to update to {version}. You’ll be asked to save any unsaved documents first."
        }
        "选择“稍后”时，会在下次退出时自动安装。" => {
            "If you choose Later, the update is installed the next time you quit."
        }
        "立即重启" => "Restart Now",
        "无法检查更新" => "Could not check for updates",
        "无法连接更新服务器，请检查网络后重试。" => {
            "Could not reach the update server. Check your connection and try again."
        }
        "下载更新失败" => "Update download failed",
        "请检查网络后重试。" => "Check your connection and try again.",
        "更新未通过校验" => "Update failed verification",
        "下载的安装包与发布签名不符，已删除，不会安装。" => {
            "The downloaded installer does not match the release signature. It was deleted and will not be installed."
        }
        "无法安装更新" => "Could not install the update",
        "可以从下载页手动安装新版本。" => {
            "You can install the new version from the download page."
        }
        "重试" => "Try Again",
        "Weave Office 更新" => "Weave Office update",
        "自动检查更新" => "Check for updates automatically",
        "立即检查" => "Check Now",
        "当前版本 {version}" => "Current version {version}",
        _ => text,
    }
}
