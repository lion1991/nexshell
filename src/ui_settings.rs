//! UI 持久化设置：加载/保存 ui_settings.json 与语言解析。

use nexshell::git_panel::{clamp_git_history_height, GIT_HISTORY_HEIGHT_DEFAULT};
use nexshell::host_management::default_database_path;

use crate::external_editor::EditorChoice;
use crate::terminal_grid_element::{
    CursorStyleChoice, GlassQualityChoice, LanguageChoice, ThemeChoice,
};

pub(crate) const TERMINAL_FONT_SIZE_DEFAULT: f32 = 14.0;
pub(crate) const TERMINAL_FONT_SIZE_MIN: f32 = 9.0;
pub(crate) const TERMINAL_FONT_SIZE_MAX: f32 = 32.0;
pub(crate) const TERMINAL_FONT_SIZE_STEP: f32 = 1.0;
pub(crate) const TERMINAL_LINE_HEIGHT_RATIO_DEFAULT: f32 = 1.2;
pub(crate) const TERMINAL_LINE_HEIGHT_RATIO_MIN: f32 = 0.5;
pub(crate) const TERMINAL_LINE_HEIGHT_RATIO_MAX: f32 = 5.0;
#[allow(dead_code)]
pub(crate) const TERMINAL_LINE_HEIGHT_RATIO_STEP: f32 = 0.1;

fn ui_settings_path() -> Option<std::path::PathBuf> {
    default_database_path().map(|p| p.with_file_name("ui_settings.json"))
}

pub(crate) struct UiSettings {
    pub sidebar_open: bool,
    pub theme: ThemeChoice,
    pub font_size: f32,
    pub line_height_ratio: f32,
    pub git_history_height: f32,
    pub opacity: u8,
    pub glass_quality: GlassQualityChoice,
    pub cursor_style: CursorStyleChoice,
    pub font_family: String,
    pub font_weight: warpui::fonts::Weight,
    pub language: LanguageChoice,
    /// 文件面板「编辑」用的编辑器。
    pub open_file_editor: EditorChoice,
    /// diff 与代码查看器「复用单标签」开关（ADR 0002，默认开启）。
    pub reuse_view_tab: bool,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            sidebar_open: false,
            theme: ThemeChoice::Dark,
            font_size: TERMINAL_FONT_SIZE_DEFAULT,
            line_height_ratio: TERMINAL_LINE_HEIGHT_RATIO_DEFAULT,
            git_history_height: GIT_HISTORY_HEIGHT_DEFAULT,
            opacity: 100,
            glass_quality: GlassQualityChoice::Frosted,
            cursor_style: CursorStyleChoice::Block,
            font_family: super::default_monospace_font_family_name(),
            font_weight: warpui::fonts::Weight::Normal,
            language: LanguageChoice::Auto,
            open_file_editor: EditorChoice::SystemDefault,
            reuse_view_tab: true,
        }
    }
}

pub(crate) fn load_ui_settings() -> UiSettings {
    let Some(path) = ui_settings_path() else {
        return UiSettings::default();
    };
    let v = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .unwrap_or(serde_json::Value::Null);
    ui_settings_from_value(v)
}

fn ui_settings_from_value(v: serde_json::Value) -> UiSettings {
    UiSettings {
        sidebar_open: v
            .get("sidebar_open")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        theme: v
            .get("theme")
            .and_then(|x| x.as_str())
            .and_then(ThemeChoice::from_id)
            .unwrap_or(ThemeChoice::Dark),
        font_size: v
            .get("font_size")
            .and_then(|x| x.as_f64())
            .map(|x| x as f32)
            .unwrap_or(TERMINAL_FONT_SIZE_DEFAULT),
        line_height_ratio: v
            .get("line_height_ratio")
            .and_then(|x| x.as_f64())
            .map(|x| {
                (x as f32).clamp(
                    TERMINAL_LINE_HEIGHT_RATIO_MIN,
                    TERMINAL_LINE_HEIGHT_RATIO_MAX,
                )
            })
            .unwrap_or(TERMINAL_LINE_HEIGHT_RATIO_DEFAULT),
        git_history_height: v
            .get("git_history_height")
            .and_then(|x| x.as_f64())
            .map(|x| clamp_git_history_height(x as f32))
            .unwrap_or(GIT_HISTORY_HEIGHT_DEFAULT),
        opacity: v
            .get("opacity")
            .and_then(|x| x.as_u64())
            .map(|x| (x as u8).clamp(1, 100))
            .unwrap_or(100),
        glass_quality: v
            .get("glass_quality")
            .and_then(|x| x.as_str())
            .and_then(GlassQualityChoice::from_id)
            .unwrap_or_default(),
        cursor_style: match v.get("cursor_style").and_then(|x| x.as_str()) {
            Some("beam") => CursorStyleChoice::Beam,
            Some("underline") => CursorStyleChoice::Underline,
            _ => CursorStyleChoice::Block,
        },
        font_family: v
            .get("font_family")
            .and_then(|x| x.as_str())
            .map(str::to_string)
            .unwrap_or_else(super::default_monospace_font_family_name),
        font_weight: match v.get("font_weight").and_then(|x| x.as_str()) {
            Some("bold") => warpui::fonts::Weight::Bold,
            _ => warpui::fonts::Weight::Normal,
        },
        language: v
            .get("language")
            .and_then(|x| x.as_str())
            .and_then(LanguageChoice::from_id)
            .unwrap_or(LanguageChoice::Auto),
        open_file_editor: v
            .get("open_file_editor")
            .and_then(|x| x.as_str())
            .and_then(EditorChoice::from_id)
            .unwrap_or(EditorChoice::SystemDefault),
        reuse_view_tab: v
            .get("reuse_view_tab")
            .and_then(|x| x.as_bool())
            .unwrap_or(true),
    }
}

pub(crate) fn save_ui_settings_to_disk(settings: &UiSettings) {
    let Some(path) = ui_settings_path() else {
        return;
    };
    let mut obj = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            if let serde_json::Value::Object(m) = v {
                Some(m)
            } else {
                None
            }
        })
        .unwrap_or_default();
    write_ui_settings_to_object(&mut obj, settings);
    // 序列化失败时保留原配置，绝不写空串覆盖
    let json = match serde_json::to_string_pretty(&obj) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[nexshell] 序列化 UI 设置失败，保留原配置: {e}");
            return;
        }
    };
    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("[nexshell] 创建配置目录失败: {e}");
            return;
        }
    }
    // 先写临时文件再 rename，避免写到一半崩溃损坏原配置
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, &json) {
        eprintln!("[nexshell] 写 UI 设置失败: {e}");
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        eprintln!("[nexshell] 替换 UI 设置失败: {e}");
        let _ = std::fs::remove_file(&tmp);
    }
}

fn write_ui_settings_to_object(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    settings: &UiSettings,
) {
    obj.insert(
        "sidebar_open".to_string(),
        serde_json::Value::Bool(settings.sidebar_open),
    );
    obj.insert(
        "theme".to_string(),
        serde_json::Value::String(settings.theme.id().to_string()),
    );
    obj.insert(
        "font_size".to_string(),
        serde_json::json!(settings.font_size),
    );
    obj.insert(
        "line_height_ratio".to_string(),
        serde_json::json!(settings.line_height_ratio),
    );
    obj.insert(
        "git_history_height".to_string(),
        serde_json::json!(settings.git_history_height),
    );
    obj.insert("opacity".to_string(), serde_json::json!(settings.opacity));
    obj.insert(
        "glass_quality".to_string(),
        serde_json::Value::String(settings.glass_quality.id().to_string()),
    );
    obj.insert(
        "cursor_style".to_string(),
        serde_json::Value::String(
            match settings.cursor_style {
                CursorStyleChoice::Block => "block",
                CursorStyleChoice::Beam => "beam",
                CursorStyleChoice::Underline => "underline",
            }
            .to_string(),
        ),
    );
    obj.insert(
        "font_family".to_string(),
        serde_json::Value::String(settings.font_family.clone()),
    );
    obj.insert(
        "font_weight".to_string(),
        serde_json::Value::String(
            match settings.font_weight {
                warpui::fonts::Weight::Bold => "bold",
                _ => "normal",
            }
            .to_string(),
        ),
    );
    obj.insert(
        "language".to_string(),
        serde_json::Value::String(settings.language.id().to_string()),
    );
    obj.insert(
        "open_file_editor".to_string(),
        serde_json::Value::String(settings.open_file_editor.id()),
    );
    obj.insert(
        "reuse_view_tab".to_string(),
        serde_json::Value::Bool(settings.reuse_view_tab),
    );
}

pub(crate) fn resolve_locale(choice: LanguageChoice) -> &'static str {
    match choice {
        LanguageChoice::English => "en",
        LanguageChoice::Chinese => "zh-CN",
        LanguageChoice::Auto => {
            let sys = sys_locale::get_locale().unwrap_or_default();
            if sys.starts_with("zh") {
                "zh-CN"
            } else {
                "en"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_grid_element::GlassQualityChoice;

    #[test]
    fn ui_settings_value_defaults_glass_quality_to_frosted() {
        let settings = ui_settings_from_value(serde_json::json!({}));

        assert_eq!(settings.glass_quality, GlassQualityChoice::Frosted);
    }

    #[test]
    fn ui_settings_value_loads_liquid_glass_quality() {
        let settings = ui_settings_from_value(serde_json::json!({
            "glass_quality": "liquid"
        }));

        assert_eq!(settings.glass_quality, GlassQualityChoice::Liquid);
    }

    #[test]
    fn write_ui_settings_inserts_top_level_glass_quality() {
        let mut settings = UiSettings::default();
        settings.glass_quality = GlassQualityChoice::Off;

        let mut obj = serde_json::Map::new();
        write_ui_settings_to_object(&mut obj, &settings);

        assert_eq!(
            obj.get("glass_quality").and_then(|v| v.as_str()),
            Some("off")
        );
    }
}
