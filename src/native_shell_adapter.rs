use crate::actions::{ShellAction, ShellEffect};
use crate::frame_export;
use crate::ipc_dispatcher::{IpcDispatchError, IpcRuntimeInputs};
use crate::layout::Size;
use crate::native_shell_host::{NativeShellFrame, NativeShellHost};
use crate::runtime_settings::NativeShellRuntimeSettings;
use crate::view_model::ShellViewSnapshot;
use crate::ShellModel;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct NativeShellAdapterFrame {
    shell: NativeShellFrame,
    frontend_frame: Value,
}

impl NativeShellAdapterFrame {
    pub fn shell_frame(&self) -> &NativeShellFrame {
        &self.shell
    }

    pub fn view(&self) -> &ShellViewSnapshot {
        &self.shell.view
    }

    pub fn frontend_frame(&self) -> &Value {
        &self.frontend_frame
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NativeShellAdapter {
    host: NativeShellHost,
    runtime: NativeShellRuntimeSettings,
}

impl NativeShellAdapter {
    pub fn new(shell: ShellModel) -> Self {
        Self::with_runtime(shell, NativeShellRuntimeSettings::default())
    }

    pub fn with_runtime(shell: ShellModel, runtime: NativeShellRuntimeSettings) -> Self {
        Self {
            host: NativeShellHost::new(shell),
            runtime,
        }
    }

    pub fn dispatch(&mut self, action: ShellAction) -> ShellEffect {
        self.host.dispatch(action)
    }

    pub fn set_runtime_settings(&mut self, runtime: NativeShellRuntimeSettings) {
        self.runtime = runtime;
    }

    pub fn render(&mut self, size: Size) -> Result<NativeShellAdapterFrame, IpcDispatchError> {
        let shell = self.host.render_frame(size);
        let frontend_frame = frame_export::resolve_frame(&shell, self.runtime.ipc_inputs())?;

        Ok(NativeShellAdapterFrame {
            shell,
            frontend_frame,
        })
    }

    pub fn render_with_runtime(
        &mut self,
        size: Size,
        runtime: &IpcRuntimeInputs,
    ) -> Result<NativeShellAdapterFrame, IpcDispatchError> {
        let shell = self.host.render_frame(size);
        let frontend_frame = frame_export::resolve_frame(&shell, runtime)?;

        Ok(NativeShellAdapterFrame {
            shell,
            frontend_frame,
        })
    }
}
