use crate::actions::{self, ShellAction, ShellEffect};
use crate::ipc_dispatcher::{self, IpcBatch};
use crate::layout::{ShellLayout, Size};
use crate::native_adapter::{self, NativeAdapterPlan, NativeAdapterState};
use crate::terminal_lifecycle::SurfaceSize;
use crate::view_model::{self, ShellViewSnapshot};
use crate::ShellModel;

#[derive(Clone, Debug, PartialEq)]
pub struct NativeShellFrame {
    pub view: ShellViewSnapshot,
    pub plan: NativeAdapterPlan,
    pub ipc: IpcBatch,
}

impl NativeShellFrame {
    pub fn lifecycle_command_names(&self) -> Vec<&'static str> {
        self.plan
            .lifecycle
            .iter()
            .map(|command| command.tauri_command_name())
            .collect()
    }

    pub fn renderer_command_names(&self) -> Vec<&'static str> {
        self.plan
            .renderer
            .iter()
            .map(|command| command.tauri_command_name())
            .collect()
    }

    pub fn ipc_command_names(&self) -> Vec<&'static str> {
        self.ipc.command_names()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NativeShellHost {
    shell: ShellModel,
    adapter_state: NativeAdapterState,
}

impl NativeShellHost {
    pub fn new(shell: ShellModel) -> Self {
        Self {
            shell,
            adapter_state: NativeAdapterState::default(),
        }
    }

    pub fn shell(&self) -> &ShellModel {
        &self.shell
    }

    pub fn dispatch(&mut self, action: ShellAction) -> ShellEffect {
        actions::reduce(&mut self.shell, action)
    }

    pub fn render_frame(&mut self, size: Size) -> NativeShellFrame {
        let layout = ShellLayout::for_window(size);
        let view = view_model::project(&self.shell, layout);
        let config = native_adapter::NativeAdapterConfig::default_for_surface(SurfaceSize {
            width: size.width,
            height: size.height,
        });
        let plan = native_adapter::plan_transition(&self.adapter_state, &view, &config);
        let ipc = ipc_dispatcher::batch(&plan, &config);
        self.adapter_state = plan.next_state.clone();

        NativeShellFrame { view, plan, ipc }
    }
}
