//! English for the editor's few words; the Chinese text is the key.

pub fn tr(zh: bool, text: &str) -> &str {
    if zh {
        return text;
    }
    match text {
        "视图" => "View",
        "设计" => "Design",
        "源码" => "Source",
        "手机宽度" => "Phone width",
        "桌面宽度" => "Desktop width",
        "设计视图" => "Design view",
        "页面使用了脚本或样式表,设计视图只按行内样式显示" => {
            "This page uses scripts or style sheets; the design view shows inline styles only"
        }
        "网络图片不加载" => "Web pictures are not loaded",
        "开始" => "Home",
        "正文" => "Body",
        "标题 1" => "Heading 1",
        "标题 2" => "Heading 2",
        "标题 3" => "Heading 3",
        "引用" => "Quote",
        "字号" => "Size",
        "加粗" => "Bold",
        "斜体" => "Italic",
        "下划线" => "Underline",
        "删除线" => "Strikethrough",
        "文字颜色" => "Text color",
        "背景色" => "Highlight",
        "默认颜色" => "Automatic",
        "无背景" => "No highlight",
        "清除格式" => "Clear formatting",
        "去掉所选文字的字体、颜色和背景" => {
            "Remove fonts, colors and highlights from the selection"
        }
        "左对齐" => "Left",
        "居中" => "Center",
        "右对齐" => "Right",
        "两端对齐" => "Justify",
        "行距" => "Spacing",
        "首行缩进" => "Indent",
        other => other,
    }
}
