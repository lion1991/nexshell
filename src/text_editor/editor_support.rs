//! 替代 warp::suggestions 的本地 stub（砍 ai 后 autosuggestion ghost text 的最小接口）。

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuggestionType {
    ShellCommand,
}

#[derive(Default)]
pub struct IgnoredSuggestionsModel;

impl IgnoredSuggestionsModel {
    pub fn is_ignored(&self, _suggestion: &str, _ty: SuggestionType) -> bool {
        false
    }
}
