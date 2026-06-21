//! 替代 warp::suggestions 的本地 stub（砍 ai 后 autosuggestion ghost text 的最小接口）。

/// 被忽略建议的最小 model（原 warp::suggestions::ignored_suggestions_model）。
pub mod ignored_suggestions_model {
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
}

pub use ignored_suggestions_model::SuggestionType;

// ── 替代 warp_completer::completer::Description 的本地 stub ──
// text_editor 内实际只用到 desc.token.span.start()/end()（command x-ray 命中测试）；
// description_text/suggestion_type/a11y_text 仅为构造完整性保留，当前 crate 内未消费。

/// 字节范围 span（镜像 warp_completer::meta::Span 的最小子集）。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    start: usize,
    end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Span {
        Span { start, end }
    }

    pub fn start(&self) -> usize {
        self.start
    }

    pub fn end(&self) -> usize {
        self.end
    }
}

impl From<(usize, usize)> for Span {
    fn from((start, end): (usize, usize)) -> Span {
        Span::new(start, end)
    }
}

/// 带 span 的载荷（镜像 warp_completer::meta::Spanned）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spanned<T> {
    pub span: Span,
    pub item: T,
}

impl<T> Spanned<T> {
    pub fn new(span: Span, item: T) -> Spanned<T> {
        Spanned { span, item }
    }
}

/// command x-ray 描述（替代 warp_completer::completer::Description）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Description {
    pub token: Spanned<String>,
    pub description_text: Option<String>,
    pub suggestion_type: SuggestionType,
}

impl Description {
    pub fn a11y_text(&self) -> String {
        match &self.description_text {
            Some(text) => format!("Command inspector triggered for {}, {}", self.token.item, text),
            None => format!("Command inspector triggered for {}", self.token.item),
        }
    }
}
