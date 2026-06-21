//! 本地化自 warp::code_review::comments —— 砍掉 RTE/GitHub 同步后，code_editor 仍需的评论数据类型。
use std::fmt::{Display, Formatter};

use warp_editor::render::model::LineCount;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CommentOrigin {
    #[default]
    Native,
    #[allow(dead_code)]
    ImportedFromGitHub(ImportedCommentDetails),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedCommentDetails {
    pub author: String,
    pub github_comment_id: String,
    pub github_parent_id: Option<String>,
    pub html_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LineDiffContent {
    pub content: String,
    pub lines_added: LineCount,
    pub lines_removed: LineCount,
}

impl LineDiffContent {
    /// diff 行原文，去掉 `+`/`-` 前缀与尾换行。
    pub(crate) fn original_text(&self) -> String {
        let s = self.content.trim_end_matches('\n');
        s.strip_prefix('+')
            .or_else(|| s.strip_prefix('-'))
            .unwrap_or(s)
            .to_string()
    }

    pub(crate) fn from_content(diff_line: &str) -> Self {
        let lines_added = LineCount::from(if diff_line.starts_with('+') { 1 } else { 0 });
        let lines_removed = LineCount::from(if diff_line.starts_with('-') { 1 } else { 0 });
        Self {
            content: diff_line.to_owned(),
            lines_added,
            lines_removed,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct CommentId(uuid::Uuid);

impl CommentId {
    pub(crate) fn new() -> Self {
        CommentId(uuid::Uuid::new_v4())
    }

    #[allow(dead_code)]
    pub(crate) fn from_uuid(uuid: uuid::Uuid) -> Self {
        Self(uuid)
    }
}

impl Display for CommentId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for CommentId {
    fn default() -> Self {
        Self::new()
    }
}
