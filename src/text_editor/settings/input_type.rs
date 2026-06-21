// 输入类型(Shell/AI)。端口自 warp crates/input_classifier，砍掉 FromStr/Display 等未用项，
// 仅保留 view 层 matches_input_type / maybe_populate_intelligent_autosuggestion 需要的最小接口。
use serde::{Deserialize, Serialize};

/// 用户输入的类型。
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputType {
    /// shell 命令输入。
    #[default]
    Shell,
    /// 面向 AI 的自然语言查询。
    AI,
}

impl InputType {
    pub fn is_ai(&self) -> bool {
        matches!(self, InputType::AI)
    }
}
