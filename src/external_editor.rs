//! 文件面板「打开 / 编辑」用的外部编辑器探测与启动。
//! 参考 warp/app/src/util/file/external_editor。
//! - 「打开」：系统默认关联程序（open / xdg-open / start）。
//! - 「编辑」：EditorChoice 指定的编辑器；SystemDefault 时回退系统文本编辑器。

use std::path::Path;

use nexshell::platform::background_command;

/// 设置里选的"编辑器"。SystemDefault = 用系统默认文本编辑器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorChoice {
    SystemDefault,
    External(ExternalEditor),
}

impl Default for EditorChoice {
    fn default() -> Self {
        Self::SystemDefault
    }
}

impl EditorChoice {
    /// 持久化 id："system_default" / "ext:vscode"。
    pub fn id(self) -> String {
        match self {
            Self::SystemDefault => "system_default".to_string(),
            Self::External(e) => format!("ext:{}", e.id()),
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        if s == "system_default" {
            return Some(Self::SystemDefault);
        }
        s.strip_prefix("ext:")
            .and_then(ExternalEditor::from_id)
            .map(Self::External)
    }
}

/// 已知外部图形编辑器（探测下拉里的候选项）。参考 warp Editor 枚举，取常见子集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalEditor {
    VSCode,
    VSCodeInsiders,
    Cursor,
    Windsurf,
    Zed,
    Sublime,
}

impl ExternalEditor {
    pub const ALL: [Self; 6] = [
        Self::VSCode,
        Self::VSCodeInsiders,
        Self::Cursor,
        Self::Windsurf,
        Self::Zed,
        Self::Sublime,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::VSCode => "vscode",
            Self::VSCodeInsiders => "vscode_insiders",
            Self::Cursor => "cursor",
            Self::Windsurf => "windsurf",
            Self::Zed => "zed",
            Self::Sublime => "sublime",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.id() == s)
    }

    /// 设置 UI 显示名。
    pub fn display_name(self) -> &'static str {
        match self {
            Self::VSCode => "VS Code",
            Self::VSCodeInsiders => "VS Code Insiders",
            Self::Cursor => "Cursor",
            Self::Windsurf => "Windsurf",
            Self::Zed => "Zed",
            Self::Sublime => "Sublime Text",
        }
    }

    /// macOS 应用名（/Applications/<name>.app）：探测与 `open -a` 启动用。
    #[cfg(target_os = "macos")]
    fn macos_app_name(self) -> &'static str {
        match self {
            Self::VSCode => "Visual Studio Code",
            Self::VSCodeInsiders => "Visual Studio Code - Insiders",
            Self::Cursor => "Cursor",
            Self::Windsurf => "Windsurf",
            Self::Zed => "Zed",
            Self::Sublime => "Sublime Text",
        }
    }

    /// CLI 命令名（Linux/Windows 探测与启动用）。
    #[cfg(not(target_os = "macos"))]
    fn cli_command(self) -> &'static str {
        match self {
            Self::VSCode => "code",
            Self::VSCodeInsiders => "code-insiders",
            Self::Cursor => "cursor",
            Self::Windsurf => "windsurf",
            Self::Zed => "zed",
            Self::Sublime => "subl",
        }
    }

    fn is_installed(self) -> bool {
        #[cfg(target_os = "macos")]
        {
            let app = format!("{}.app", self.macos_app_name());
            if Path::new("/Applications").join(&app).exists() {
                return true;
            }
            std::env::var("HOME")
                .ok()
                .map(|home| Path::new(&home).join("Applications").join(&app).exists())
                .unwrap_or(false)
        }
        #[cfg(not(target_os = "macos"))]
        {
            command_in_path(self.cli_command())
        }
    }

    fn open(self, path: &str) -> Result<(), String> {
        let arg = safe_arg(path);
        #[cfg(target_os = "macos")]
        {
            spawn("open", &["-a", self.macos_app_name(), arg.as_str()])
        }
        #[cfg(not(target_os = "macos"))]
        {
            spawn(self.cli_command(), &[arg.as_str()])
        }
    }
}

/// 探测系统已安装的候选编辑器。设置下拉用。
pub fn detect_installed_editors() -> Vec<ExternalEditor> {
    ExternalEditor::ALL
        .into_iter()
        .filter(|e| e.is_installed())
        .collect()
}

/// 「打开」：系统默认关联程序。
pub fn open_path_with_default(path: &str) -> Result<(), String> {
    let arg = safe_arg(path);
    #[cfg(target_os = "macos")]
    {
        spawn("open", &[arg.as_str()])
    }
    #[cfg(target_os = "windows")]
    {
        // start 第一个引号参数是窗口标题，留空。
        spawn("cmd", &["/C", "start", "", arg.as_str()])
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        spawn("xdg-open", &[arg.as_str()])
    }
}

/// 「编辑」：按 EditorChoice 打开；SystemDefault 回退系统文本编辑器。
pub fn open_path_with_editor(choice: EditorChoice, path: &str) -> Result<(), String> {
    match choice {
        EditorChoice::SystemDefault => open_with_system_text_editor(path),
        EditorChoice::External(editor) => editor.open(path),
    }
}

fn open_with_system_text_editor(path: &str) -> Result<(), String> {
    let arg = safe_arg(path);
    #[cfg(target_os = "macos")]
    {
        spawn("open", &["-t", arg.as_str()])
    }
    #[cfg(target_os = "windows")]
    {
        spawn("notepad", &[arg.as_str()])
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Ok(editor) = std::env::var("EDITOR") {
            if !editor.is_empty() {
                return spawn(&editor, &[arg.as_str()]);
            }
        }
        spawn("xdg-open", &[arg.as_str()])
    }
}

/// 防 argv flag 注入：保证路径不以 `-` 开头被子进程当成选项。
/// 绝对路径天然安全（POSIX `/`、Windows 盘符/UNC 开头）；相对且以 `-` 开头的前缀 `./`。
fn safe_arg(path: &str) -> String {
    if Path::new(path).is_absolute() || !path.starts_with('-') {
        path.to_string()
    } else {
        format!("./{path}")
    }
}

#[cfg(not(target_os = "macos"))]
fn command_in_path(cmd: &str) -> bool {
    #[cfg(target_os = "windows")]
    let finder = "where";
    #[cfg(not(target_os = "windows"))]
    let finder = "which";
    background_command(finder)
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn spawn(program: &str, args: &[&str]) -> Result<(), String> {
    background_command(program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{program} 启动失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::safe_arg;
    use nexshell::platform::background_process_creation_flags;

    #[test]
    fn safe_arg_keeps_absolute_and_non_dash_paths() {
        assert_eq!(
            safe_arg("/Users/example/notes.txt"),
            "/Users/example/notes.txt"
        );
        assert_eq!(safe_arg("notes.txt"), "notes.txt");
        assert_eq!(safe_arg("./notes.txt"), "./notes.txt");
    }

    #[test]
    fn safe_arg_neutralizes_relative_dash_paths() {
        // 相对且以 - 开头 → 前缀 ./，避免被子进程当成 flag
        assert_eq!(safe_arg("-rf"), "./-rf");
        assert_eq!(safe_arg("--goto"), "./--goto");
    }

    #[test]
    fn background_processes_use_the_windows_no_window_flag() {
        #[cfg(target_os = "windows")]
        assert_eq!(background_process_creation_flags(), 0x0800_0000);

        #[cfg(not(target_os = "windows"))]
        assert_eq!(background_process_creation_flags(), 0);
    }
}
