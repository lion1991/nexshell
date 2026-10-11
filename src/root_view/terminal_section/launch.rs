// terminal_section::launch — 系统交来的文件 / 目录（Finder「打开方式」、其他应用 open .command）。
// 只含 impl RootView。目录开本地标签进去；文件在所在目录开本地标签，shell 就绪后执行它（同 Warp）。

use std::path::PathBuf;
use std::time::Duration;

use warpui::r#async::Timer;
use warpui::ViewContext;

use crate::file_panel_view_helpers::shell_quote_posix;
use crate::RootView;

/// 没有 shell integration 的 shell（sh / nu，或 wrapper rc 写失败）永远等不到 marker，到点直接发。
const STARTUP_INPUT_FALLBACK: Duration = Duration::from_secs(3);

impl RootView {
    pub(crate) fn open_paths_from_system(
        &mut self,
        paths: Vec<PathBuf>,
        ctx: &mut ViewContext<Self>,
    ) {
        for path in paths {
            if path.is_dir() {
                self.open_local_terminal_tab_in_dir(path, ctx);
                continue;
            }
            let Some(dir) = path.parent() else {
                continue;
            };
            self.open_local_terminal_tab_in_dir(dir.to_path_buf(), ctx);
            let command = format!("{}\r", shell_quote_posix(&path.to_string_lossy()));
            let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) else {
                continue;
            };
            tab.startup_input = Some(command.into_bytes());
            let tab_id = tab.id.clone();
            ctx.spawn(Timer::after(STARTUP_INPUT_FALLBACK), move |me, _, _| {
                let Some(tab) = me.terminal_tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                    return;
                };
                if let (Some(bytes), Ok(rt)) = (tab.startup_input.take(), tab.terminal.lock()) {
                    rt.send_input(bytes);
                }
            });
        }
        ctx.windows().show_window_and_focus_app(ctx.window_id());
    }

    /// 等 shell 发出 bootstrap marker 再发：rc 还没跑完时写进去会被 tty 回显一遍、行编辑器再显示一遍。
    pub(in crate::root_view) fn flush_startup_inputs(&mut self) {
        for tab in self
            .terminal_tabs
            .iter_mut()
            .filter(|tab| tab.startup_input.is_some())
        {
            let Ok(rt) = tab.terminal.lock() else {
                continue;
            };
            if rt.is_bootstrapped() {
                if let Some(bytes) = tab.startup_input.take() {
                    rt.send_input(bytes);
                }
            }
        }
    }
}
