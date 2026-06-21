//! 剪贴板内容转字符串(路径按 shell 转义)。从 warp app util::clipboard 端口。
use std::borrow::Cow;

use itertools::Itertools;
use warp_util::path::ShellFamily;
use warpui::clipboard::ClipboardContent;

/// 把 ClipboardContent 转成字符串;已知 shell 时转义其中的路径,否则原样。
pub fn clipboard_content_with_escaped_paths(
    mut content: ClipboardContent,
    shell_family: Option<ShellFamily>,
    replace_newlines_with_spaces: bool,
) -> String {
    if replace_newlines_with_spaces {
        content = ClipboardContent {
            plain_text: content.plain_text.replace('\n', " "),
            ..content
        }
    }
    match content.paths {
        Some(paths) => paths
            .iter()
            .map(|path| match shell_family {
                Some(shell_family) => shell_family.escape(path),
                None => Cow::Borrowed(path.as_ref()),
            })
            .join(" "),
        None => content.plain_text,
    }
}
