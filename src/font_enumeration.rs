//! 系统字体枚举与候选优先级（Warp appearance_page.rs:2010-2025 同等思路）。

use warpui::fonts;

#[cfg(target_os = "windows")]
use std::collections::BTreeMap;

#[cfg(target_os = "macos")]
pub(crate) fn enumerate_monospace_fonts() -> Vec<String> {
    use core_text::font_collection::create_for_all_families;
    use core_text::font_descriptor::{SymbolicTraitAccessors, TraitAccessors};
    let collection = create_for_all_families();
    let Some(descriptors) = collection.get_descriptors() else {
        return vec!["Menlo".to_string()];
    };
    let mut names: Vec<String> = Vec::new();
    for desc in descriptors.iter() {
        if desc.traits().symbolic_traits().is_monospace() {
            let name = desc.family_name();
            if !name.is_empty() && !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    if names.is_empty() {
        names.push("Menlo".to_string());
    }
    names
}

#[cfg(target_os = "windows")]
pub(crate) fn enumerate_monospace_fonts() -> Vec<String> {
    let fonts = enumerate_windows_system_fonts(WindowsFontListKind::Terminal);
    if fonts.is_empty() {
        return monospace_font_families()
            .iter()
            .map(|family| (*family).to_string())
            .collect();
    }
    fonts
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
pub(crate) fn enumerate_monospace_fonts() -> Vec<String> {
    monospace_font_families()
        .iter()
        .map(|family| (*family).to_string())
        .collect()
}

// warp: appearance_page.rs:2010-2025 — 枚举所有系统字体
#[cfg(target_os = "macos")]
pub(crate) fn enumerate_all_system_fonts() -> Vec<String> {
    use core_text::font_collection::create_for_all_families;
    let collection = create_for_all_families();
    let Some(descriptors) = collection.get_descriptors() else {
        return enumerate_monospace_fonts();
    };
    let mut names: Vec<String> = Vec::new();
    for desc in descriptors.iter() {
        let name = desc.family_name();
        if !name.is_empty() && !name.starts_with('.') && !names.contains(&name) {
            names.push(name);
        }
    }
    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    names
}

#[cfg(target_os = "windows")]
pub(crate) fn enumerate_all_system_fonts() -> Vec<String> {
    let fonts = enumerate_windows_system_fonts(WindowsFontListKind::All);
    if fonts.is_empty() {
        return enumerate_monospace_fonts();
    }
    fonts
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
pub(crate) fn enumerate_all_system_fonts() -> Vec<String> {
    enumerate_monospace_fonts()
}

pub(crate) fn default_monospace_font_family_name() -> String {
    #[cfg(target_os = "windows")]
    {
        return enumerate_monospace_fonts()
            .into_iter()
            .next()
            .unwrap_or_else(|| {
                monospace_font_families()
                    .first()
                    .copied()
                    .unwrap_or("Menlo")
                    .to_string()
            });
    }

    #[cfg(not(target_os = "windows"))]
    {
        monospace_font_families()
            .first()
            .copied()
            .unwrap_or("Menlo")
            .to_string()
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn prioritize_installed_font_families(
    installed_families: Vec<String>,
    preferred_order: &[&str],
) -> Vec<String> {
    let mut remaining = std::collections::BTreeMap::new();
    for family in installed_families {
        let family = family.trim();
        if family.is_empty() || family.starts_with('.') {
            continue;
        }
        remaining
            .entry(normalized_font_family_key(family))
            .or_insert_with(|| family.to_string());
    }

    let mut prioritized = Vec::new();
    for preferred in preferred_order {
        if let Some(family) = remaining.remove(&normalized_font_family_key(preferred)) {
            prioritized.push(family);
        }
    }
    prioritized.extend(remaining.into_values());
    prioritized
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn normalized_font_family_key(family: &str) -> String {
    family.trim().to_lowercase()
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy)]
enum WindowsFontListKind {
    Terminal,
    All,
}

#[cfg(target_os = "windows")]
fn enumerate_windows_system_fonts(kind: WindowsFontListKind) -> Vec<String> {
    let source = font_kit::source::SystemSource::new();
    let handles = match source.all_fonts() {
        Ok(handles) => handles,
        Err(error) => {
            log::warn!("unable to enumerate Windows system fonts: {error:?}");
            return Vec::new();
        }
    };

    let mut families = BTreeMap::<String, (String, bool)>::new();
    for handle in handles {
        let Ok(font) = handle.load() else {
            continue;
        };
        if font.glyph_for_char('m').is_none() {
            continue;
        }
        let family_name = font.family_name();
        let key = normalized_font_family_key(&family_name);
        if key.is_empty() {
            continue;
        }
        let entry = families.entry(key).or_insert((family_name, false));
        entry.1 |= font.is_monospace();
    }

    let installed = families
        .into_values()
        .filter_map(|(family, is_monospace)| match kind {
            WindowsFontListKind::All => Some(family),
            WindowsFontListKind::Terminal => {
                if is_monospace || font_family_in_list(&family, windows_monospace_font_families()) {
                    Some(family)
                } else {
                    None
                }
            }
        })
        .collect();

    let preferred_order = match kind {
        WindowsFontListKind::Terminal => windows_monospace_font_families(),
        WindowsFontListKind::All => windows_all_font_families(),
    };
    prioritize_installed_font_families(installed, preferred_order)
}

#[cfg(target_os = "windows")]
fn font_family_in_list(family: &str, families: &[&str]) -> bool {
    let family = normalized_font_family_key(family);
    families
        .iter()
        .any(|candidate| normalized_font_family_key(candidate) == family)
}

#[cfg(target_os = "windows")]
fn windows_all_font_families() -> &'static [&'static str] {
    &[
        "Microsoft YaHei UI",
        "Microsoft YaHei",
        "NSimSun",
        "SimSun",
        "DengXian",
        "Noto Sans CJK SC",
        "Noto Sans Mono CJK SC",
        "Sarasa Mono SC",
        "Cascadia Mono",
        "Consolas",
        "Courier New",
        "Segoe UI",
        "Arial",
    ]
}

#[cfg(target_os = "macos")]
pub(crate) fn monospace_font_families() -> &'static [&'static str] {
    &["Menlo", "Monaco"]
}

#[cfg(target_os = "windows")]
pub(crate) fn monospace_font_families() -> &'static [&'static str] {
    windows_monospace_font_families()
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
pub(crate) fn monospace_font_families() -> &'static [&'static str] {
    &[
        "Cascadia Mono",
        "Consolas",
        "Courier New",
        "Menlo",
        "Monaco",
    ]
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn windows_monospace_font_families() -> &'static [&'static str] {
    &[
        "NSimSun",
        "Microsoft YaHei Mono",
        "Microsoft YaHei UI",
        "Microsoft YaHei",
        "SimSun",
        "DengXian",
        "Noto Sans Mono CJK SC",
        "Sarasa Mono SC",
        "Cascadia Mono",
        "Consolas",
        "Courier New",
        "Menlo",
        "Monaco",
    ]
}

#[cfg(target_os = "windows")]
pub(crate) fn ui_font_families() -> &'static [&'static str] {
    windows_ui_font_families()
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn ui_font_families() -> &'static [&'static str] {
    &["Helvetica Neue", "Helvetica", "Arial", "Segoe UI"]
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn windows_ui_font_families() -> &'static [&'static str] {
    &[
        "Microsoft YaHei UI",
        "Microsoft YaHei",
        "DengXian",
        "SimSun",
        "Noto Sans CJK SC",
        "Segoe UI",
        "Arial",
    ]
}

pub(crate) fn load_nexshell_monospace_font(
    cache: &mut fonts::Cache,
    preferred_family: Option<&str>,
) -> fonts::FamilyId {
    let preferred_family = preferred_family
        .map(str::trim)
        .filter(|family| !family.is_empty());

    for family in preferred_family
        .into_iter()
        .chain(monospace_font_families().iter().copied())
    {
        if let Ok(font) = cache.get_or_load_system_font(family) {
            return font;
        }
    }

    cache
        .load_family_from_bytes(
            "Hack",
            vec![
                include_bytes!("../assets/bundled/fonts/hack/Hack-Italic.ttf").to_vec(),
                include_bytes!("../assets/bundled/fonts/hack/Hack-Bold.ttf").to_vec(),
                include_bytes!("../assets/bundled/fonts/hack/Hack-Regular.ttf").to_vec(),
                include_bytes!("../assets/bundled/fonts/hack/Hack-BoldItalic.ttf").to_vec(),
            ],
        )
        .expect("bundled Hack font")
}

pub(crate) fn load_nexshell_ui_font(cache: &mut fonts::Cache) -> Option<fonts::FamilyId> {
    for family in ui_font_families() {
        if let Ok(font) = cache.get_or_load_system_font(family) {
            return Some(font);
        }
    }

    cache
        .load_family_from_bytes(
            "Roboto",
            vec![
                include_bytes!("../assets/bundled/fonts/roboto/Roboto-Italic.ttf").to_vec(),
                include_bytes!("../assets/bundled/fonts/roboto/Roboto-Bold.ttf").to_vec(),
                include_bytes!("../assets/bundled/fonts/roboto/Roboto-Regular.ttf").to_vec(),
                include_bytes!("../assets/bundled/fonts/roboto/Roboto-Medium.ttf").to_vec(),
                include_bytes!("../assets/bundled/fonts/roboto/RobotoFlex-Semibold.ttf").to_vec(),
                include_bytes!("../assets/bundled/fonts/roboto/Roboto-BoldItalic.ttf").to_vec(),
            ],
        )
        .ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn windows_font_candidates_include_cjk_families_first() {
        let monospace = super::windows_monospace_font_families();
        assert!(monospace
            .iter()
            .take(3)
            .any(|family| family.contains("YaHei") || family.contains("SimSun")));

        let ui = super::windows_ui_font_families();
        assert!(ui[0].contains("YaHei"));
    }

    #[test]
    fn available_windows_fonts_only_include_detected_families_with_cjk_first() {
        let installed = vec![
            "Consolas".to_string(),
            "Fira Code".to_string(),
            "Microsoft YaHei".to_string(),
        ];

        let fonts = super::prioritize_installed_font_families(
            installed,
            super::windows_monospace_font_families(),
        );

        assert_eq!(
            fonts,
            vec![
                "Microsoft YaHei".to_string(),
                "Consolas".to_string(),
                "Fira Code".to_string(),
            ]
        );
        assert!(!fonts.contains(&"NSimSun".to_string()));
    }
}
