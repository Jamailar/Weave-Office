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
        "设计视图暂不能直接编辑,请切换到源码修改" => {
            "Editing in the design view is coming; switch to Source to edit"
        }
        other => other,
    }
}
