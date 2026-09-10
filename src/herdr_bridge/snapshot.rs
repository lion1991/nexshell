//! `session.snapshot` 的索引与查询。纯数据 + 纯函数，无 I/O。
//!
//! herdr 的焦点是 **per-client** 的（官方 concepts：多 client 时各看各的
//! workspace / tab），而 socket API 的 `focused_*` 只反映「前台 client」。
//! nexshell 里的 client 切 workspace 时，若别处还挂着另一个 client，
//! server 的全局焦点根本不动。所以这里不能只看 `focused_*`，而是靠
//! **窗口标题反查 workspace**（herdr client 会往宿主 pty 写 OSC 0/2，
//! 默认模板 `{hostname}: {workspace}`），再从该 workspace 取焦点 pane 的 cwd。

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

/// label 反查结果。label 是目录 basename，**可能重名**（本机就有两个
/// `impl-tools`）。重名不等于有歧义：`resolve_cwd` 会先比对这些 workspace
/// 的焦点目录，只有目录确实不同才回退全局焦点。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    Unique(String),
    Ambiguous,
    NotFound,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct WorkspaceInfo {
    #[serde(default)]
    workspace_id: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    active_tab_id: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct LayoutInfo {
    #[serde(default)]
    tab_id: String,
    #[serde(default)]
    focused_pane_id: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PaneInfo {
    #[serde(default)]
    pane_id: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    foreground_cwd: Option<String>,
}

impl PaneInfo {
    /// 以 `cwd` 为准，`foreground_cwd` 仅 fallback。
    fn panel_cwd(&self) -> Option<PathBuf> {
        non_empty(self.cwd.as_deref())
            .or_else(|| non_empty(self.foreground_cwd.as_deref()))
            .map(PathBuf::from)
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|s| !s.trim().is_empty())
}

/// 一份 snapshot 的可查索引。相等比较用于判断「拉回来的内容有没有变」。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SnapshotIndex {
    /// workspace_id → 该 workspace 焦点 pane 的 cwd。
    workspace_cwd: HashMap<String, PathBuf>,
    /// label → 拥有该 label 的 workspace_id 列表（重名时长度 > 1）。
    label_to_workspaces: HashMap<String, Vec<String>>,
    /// server 全局焦点 pane 的 cwd（= 前台 client 的焦点）。
    global_cwd: Option<PathBuf>,
}

impl SnapshotIndex {
    /// 从 `result.snapshot` 的 JSON 建索引。字段缺失一律跳过，不 panic。
    pub fn from_json(snapshot: &Value) -> Self {
        let workspaces: Vec<WorkspaceInfo> = deserialize_array(snapshot.get("workspaces"));
        let layouts: Vec<LayoutInfo> = deserialize_array(snapshot.get("layouts"));
        let panes: Vec<PaneInfo> = deserialize_array(snapshot.get("panes"));

        let pane_cwd: HashMap<&str, PathBuf> = panes
            .iter()
            .filter(|p| !p.pane_id.is_empty())
            .filter_map(|p| Some((p.pane_id.as_str(), p.panel_cwd()?)))
            .collect();
        // 空 tab_id / 空 active_tab_id 都可能出现在半初始化的 workspace 上，
        // 不过滤就会用空串互相误配，把无关 workspace 指到别人的 pane。
        let tab_focus: HashMap<&str, &str> = layouts
            .iter()
            .filter(|l| !l.tab_id.is_empty() && !l.focused_pane_id.is_empty())
            .map(|l| (l.tab_id.as_str(), l.focused_pane_id.as_str()))
            .collect();

        let mut workspace_cwd = HashMap::new();
        let mut label_to_workspaces: HashMap<String, Vec<String>> = HashMap::new();
        for ws in &workspaces {
            if ws.workspace_id.is_empty() {
                continue;
            }
            if !ws.label.is_empty() {
                label_to_workspaces
                    .entry(ws.label.clone())
                    .or_default()
                    .push(ws.workspace_id.clone());
            }
            if ws.active_tab_id.is_empty() {
                continue;
            }
            // workspace → active_tab_id → layout.focused_pane_id → pane.cwd
            if let Some(cwd) = tab_focus
                .get(ws.active_tab_id.as_str())
                .and_then(|pane_id| pane_cwd.get(pane_id))
            {
                workspace_cwd.insert(ws.workspace_id.clone(), cwd.clone());
            }
        }

        let global_cwd = snapshot
            .get("focused_pane_id")
            .and_then(Value::as_str)
            .and_then(|id| pane_cwd.get(id))
            .cloned();

        Self {
            workspace_cwd,
            label_to_workspaces,
            global_cwd,
        }
    }

    /// server 全局焦点 pane 的 cwd。标题反查不出来时的回退。
    pub fn global_focused_cwd(&self) -> Option<PathBuf> {
        self.global_cwd.clone()
    }

    /// 按 label（目录 basename）反查 workspace。重名不猜。
    pub fn resolve_workspace_by_label(&self, label: &str) -> Resolution {
        match self.label_to_workspaces.get(label) {
            None => Resolution::NotFound,
            Some(ids) if ids.len() == 1 => Resolution::Unique(ids[0].clone()),
            Some(_) => Resolution::Ambiguous,
        }
    }

    /// 指定 workspace 的焦点 pane cwd。
    pub fn workspace_focused_cwd(&self, workspace_id: &str) -> Option<PathBuf> {
        self.workspace_cwd.get(workspace_id).cloned()
    }

    /// 完整解析：标题 → label → workspace → cwd。
    ///
    /// 标题里**完全没有 `": "`** 时返回 `None`（而不是回退全局焦点）：那多半是
    /// herdr 还没接管、pty 里留着上一个 shell 写的标题，此时用全局焦点会把面板
    /// 拽到别的 client 的目录去；返回 None 让上层退回 OSC 7 的 `local_cwd` 更稳。
    /// 代价是用户把 `ui.window_title` 改成只剩 `{workspace}`（没有分隔符）时反查
    /// 失效——但那种配置本来也只能回退，不算退化。
    ///
    /// 有分隔符但 label 认不出 / 重名且目录不同时，仍回退全局焦点。
    pub fn resolve_cwd(&self, title: Option<&str>) -> Option<PathBuf> {
        let Some(title) = title else {
            return self.global_focused_cwd();
        };
        if !title.contains(TITLE_SEPARATOR) {
            return None;
        }
        let Some(label) = workspace_label_from_title(title) else {
            return self.global_focused_cwd();
        };
        match self.resolve_workspace_by_label(label) {
            Resolution::Unique(id) => self
                .workspace_focused_cwd(&id)
                .or_else(|| self.global_focused_cwd()),
            // 重名不一定有歧义：同一个仓库开多个 workspace 时目录其实一样。
            Resolution::Ambiguous => self
                .unambiguous_cwd_for_label(label)
                .or_else(|| self.global_focused_cwd()),
            // 认不出来：宁可用全局焦点，也不猜错目录。
            Resolution::NotFound => self.global_focused_cwd(),
        }
    }

    /// 重名 workspace 的焦点目录去重。全都指向同一个目录就没有歧义，直接用；
    /// 目录确实不同（或全都取不到）才返回 None 交给调用方回退全局焦点。
    /// 取不到 cwd 的候选直接跳过，不影响其余候选的判定。
    fn unambiguous_cwd_for_label(&self, label: &str) -> Option<PathBuf> {
        let ids = self.label_to_workspaces.get(label)?;
        let mut only: Option<&PathBuf> = None;
        for id in ids {
            let Some(cwd) = self.workspace_cwd.get(id) else {
                continue;
            };
            match only {
                None => only = Some(cwd),
                Some(seen) if seen == cwd => {}
                // 目录不同 → 真歧义，不猜。
                Some(_) => return None,
            }
        }
        only.cloned()
    }
}

fn deserialize_array<T: for<'de> Deserialize<'de>>(value: Option<&Value>) -> Vec<T> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| serde_json::from_value(item.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// herdr 默认窗口标题模板 `{hostname}: {workspace}` 里的分隔符。
pub const TITLE_SEPARATOR: &str = ": ";

/// 从 herdr client 写给宿主 pty 的窗口标题里取 workspace label。
///
/// 按**第一个** `": "` 切、取右半（hostname 里带冒号也不会切错）。没有分隔符
/// 或右半为空时返回 None —— 调用方据此区分「不是 herdr 写的标题」。
pub fn workspace_label_from_title(title: &str) -> Option<&str> {
    let (_, label) = title.split_once(TITLE_SEPARATOR)?;
    let label = label.trim();
    (!label.is_empty()).then_some(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真机 `herdr api snapshot` 裁剪而来（保留重名 label `impl-tools`、
    /// 一个 focused_pane 不是 p1 的 workspace、一个 cwd 为 null 的 pane）。
    const FIXTURE: &str = include_str!("testdata/session_snapshot.json");

    fn index() -> SnapshotIndex {
        let envelope: Value = serde_json::from_str(FIXTURE).unwrap();
        SnapshotIndex::from_json(&envelope["result"]["snapshot"])
    }

    #[test]
    fn indexes_every_workspace_from_the_real_fixture() {
        let idx = index();
        assert_eq!(
            idx.workspace_focused_cwd("w12"),
            Some(PathBuf::from(
                "/Users/matt/SynologyDrive/code/vpstools/nexshell"
            ))
        );
        // wS 的焦点 pane 是 p2，不是 p1 —— 必须走 layouts 而不是猜。
        assert_eq!(
            idx.workspace_focused_cwd("wS"),
            Some(PathBuf::from("/Users/matt/SynologyDrive/code/work/taskops"))
        );
        assert_eq!(idx.workspace_focused_cwd("nope"), None);
    }

    #[test]
    fn null_cwd_falls_back_to_foreground_cwd() {
        // wG 的 pane cwd 是 null，只有 foreground_cwd。
        assert_eq!(
            index().workspace_focused_cwd("wG"),
            Some(PathBuf::from("/Users/matt/SynologyDrive/code/tweaks"))
        );
    }

    #[test]
    fn global_focused_cwd_comes_from_focused_pane_id() {
        assert_eq!(
            index().global_focused_cwd(),
            Some(PathBuf::from(
                "/Users/matt/SynologyDrive/code/vpstools/nexshell"
            ))
        );
    }

    #[test]
    fn resolves_unique_label() {
        assert_eq!(
            index().resolve_workspace_by_label("taskops"),
            Resolution::Unique("wS".to_string())
        );
    }

    #[test]
    fn duplicate_labels_are_ambiguous() {
        // 本机真的有两个 impl-tools（w11 / w14）。
        assert_eq!(
            index().resolve_workspace_by_label("impl-tools"),
            Resolution::Ambiguous
        );
    }

    #[test]
    fn unknown_label_is_not_found() {
        assert_eq!(
            index().resolve_workspace_by_label("nothing-here"),
            Resolution::NotFound
        );
    }

    #[test]
    fn resolve_cwd_uses_the_titles_workspace() {
        let idx = index();
        assert_eq!(
            idx.resolve_cwd(Some("Matts-MacBook-Pro: taskops")),
            Some(PathBuf::from("/Users/matt/SynologyDrive/code/work/taskops"))
        );
        assert_eq!(
            idx.resolve_cwd(Some("Matts-MacBook-Pro: tweaks")),
            Some(PathBuf::from("/Users/matt/SynologyDrive/code/tweaks"))
        );
    }

    /// 重名但两个 workspace 的焦点目录一样（本机 w11/w14 impl-tools 就是这样）
    /// → 没有歧义，直接采用，不必回退全局焦点。
    #[test]
    fn duplicate_labels_with_identical_cwd_are_adopted() {
        let idx = index();
        assert_eq!(
            idx.resolve_workspace_by_label("impl-tools"),
            Resolution::Ambiguous
        );
        assert_eq!(
            idx.resolve_cwd(Some("Matts-MacBook-Pro: impl-tools")),
            Some(PathBuf::from(
                "/Users/matt/SynologyDrive/code/work/impl-tools"
            ))
        );
    }

    /// 重名且目录确实不同 → 真歧义，回退全局焦点。
    #[test]
    fn duplicate_labels_with_different_cwd_fall_back_to_global() {
        let idx = index();
        assert_eq!(
            idx.resolve_workspace_by_label("Proxy"),
            Resolution::Ambiguous
        );
        assert_eq!(
            idx.resolve_cwd(Some("host: Proxy")),
            idx.global_focused_cwd()
        );
    }

    /// 重名且其中一个取不到 cwd → 按剩下的非 None 去重，仍然唯一就采用。
    #[test]
    fn duplicate_labels_ignore_candidates_without_a_cwd() {
        let idx = index();
        assert_eq!(idx.workspace_focused_cwd("wZ1"), None);
        assert_eq!(
            idx.resolve_workspace_by_label("qemUI"),
            Resolution::Ambiguous
        );
        assert_eq!(
            idx.resolve_cwd(Some("host: qemUI")),
            Some(PathBuf::from(
                "/Users/matt/SynologyDrive/code/work/tools/qemUI"
            ))
        );
    }

    /// 重名且候选全都取不到 cwd → 回退全局焦点。
    #[test]
    fn duplicate_labels_with_no_cwd_at_all_fall_back_to_global() {
        let mut idx = index();
        idx.workspace_cwd.remove("wZ2");
        assert_eq!(
            idx.resolve_cwd(Some("host: qemUI")),
            idx.global_focused_cwd()
        );
    }

    #[test]
    fn resolve_cwd_falls_back_to_global_on_unknown_or_missing_title() {
        let idx = index();
        let global = Some(PathBuf::from(
            "/Users/matt/SynologyDrive/code/vpstools/nexshell",
        ));
        // 有分隔符但 label 认不出来 → 仍回退全局焦点。
        assert_eq!(idx.resolve_cwd(Some("host: something-else")), global);
        // 有分隔符但右半为空。
        assert_eq!(idx.resolve_cwd(Some("host: ")), global);
        // 压根没标题（从没收到过 OSC 0/2）。
        assert_eq!(idx.resolve_cwd(None), global);
        // 空标题 / 全空白：没有分隔符 → None。
        assert_eq!(idx.resolve_cwd(Some("")), None);
        assert_eq!(idx.resolve_cwd(Some("   ")), None);
    }

    /// 空串 id 不能互相误配：半初始化的 workspace / layout 会带空 tab_id。
    #[test]
    fn empty_ids_never_cross_match() {
        let snapshot = serde_json::json!({
            "focused_pane_id": "",
            "workspaces": [
                { "workspace_id": "wA", "label": "half-init", "active_tab_id": "" },
                { "workspace_id": "wB", "label": "ok", "active_tab_id": "wB:t1" }
            ],
            "layouts": [
                { "workspace_id": "wZ", "tab_id": "", "focused_pane_id": "ghost:p1" },
                { "workspace_id": "wB", "tab_id": "wB:t1", "focused_pane_id": "wB:p1" }
            ],
            "panes": [
                { "pane_id": "", "cwd": "/ghost" },
                { "pane_id": "ghost:p1", "cwd": "/ghost" },
                { "pane_id": "wB:p1", "cwd": "/real" }
            ]
        });
        let idx = SnapshotIndex::from_json(&snapshot);
        // active_tab_id 为空的 workspace 不该借空 tab_id 的 layout 配到 /ghost。
        assert_eq!(idx.workspace_focused_cwd("wA"), None);
        assert_eq!(
            idx.workspace_focused_cwd("wB"),
            Some(PathBuf::from("/real"))
        );
        // focused_pane_id 为空串也不该配到 pane_id 为空串的那条。
        assert_eq!(idx.global_focused_cwd(), None);
        assert_eq!(idx.resolve_cwd(Some("host: half-init")), None);
        assert_eq!(
            idx.resolve_cwd(Some("host: ok")),
            Some(PathBuf::from("/real"))
        );
    }

    #[test]
    fn label_is_taken_after_the_first_colon_space() {
        assert_eq!(
            workspace_label_from_title("Matts-MacBook-Pro: nexshell"),
            Some("nexshell")
        );
        // hostname 里带冒号：只按第一个 ": " 切，右半原样保留。
        assert_eq!(
            workspace_label_from_title("host:weird: my: project"),
            Some("my: project")
        );
        // 两侧空白 trim 掉。
        assert_eq!(
            workspace_label_from_title("host:   nexshell  "),
            Some("nexshell")
        );
        // 没有分隔符 / 右半为空 → 不是 herdr 写的标题。
        assert_eq!(workspace_label_from_title("nexshell"), None);
        assert_eq!(workspace_label_from_title("host:nospace"), None);
        assert_eq!(workspace_label_from_title(""), None);
        assert_eq!(workspace_label_from_title("   "), None);
        assert_eq!(workspace_label_from_title("host: "), None);
    }

    /// 没有 ": " 的标题多半是 herdr 还没接管、上一个 shell 留下的。
    /// 返回 None 让上层退回 OSC 7 的 local_cwd，别拽到别的 client 的目录去。
    #[test]
    fn title_without_a_separator_yields_none_not_the_global_focus() {
        let idx = index();
        assert!(idx.global_focused_cwd().is_some());
        for title in [
            "nexshell",
            "matt@Matts-MacBook-Pro",
            "~/code/nexshell",
            "-zsh",
        ] {
            assert_eq!(idx.resolve_cwd(Some(title)), None, "{title}");
        }
    }

    #[test]
    fn malformed_snapshot_yields_an_empty_index() {
        let idx = SnapshotIndex::from_json(&serde_json::json!({}));
        assert_eq!(idx.global_focused_cwd(), None);
        assert_eq!(idx.resolve_cwd(Some("host: x")), None);
        assert_eq!(idx.resolve_workspace_by_label("x"), Resolution::NotFound);

        let idx = SnapshotIndex::from_json(&serde_json::json!({
            "workspaces": "not-an-array", "panes": 42, "layouts": null
        }));
        assert_eq!(idx, SnapshotIndex::default());
    }

    #[test]
    fn index_equality_detects_content_changes() {
        let a = index();
        let b = index();
        assert_eq!(a, b);
        assert_ne!(a, SnapshotIndex::default());
    }
}
