use crate::native_adapter::{NativeAdapterConfig, NativeAdapterPlan};
use crate::renderer_ipc::RendererIpcCommand;
use crate::terminal_lifecycle::TerminalLifecycleCommand;
use serde_json::{Map, Number, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct IpcBatch {
    pub calls: Vec<IpcCall>,
}

impl IpcBatch {
    pub fn command_names(&self) -> Vec<&'static str> {
        self.calls.iter().map(|call| call.command).collect()
    }

    pub fn find(&self, command: &'static str) -> Option<&IpcCall> {
        self.calls.iter().find(|call| call.command == command)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct IpcCall {
    pub command: &'static str,
    pub args: Vec<IpcArg>,
}

impl IpcCall {
    fn new(command: &'static str, args: Vec<IpcArg>) -> Self {
        Self { command, args }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct IpcArg {
    pub name: &'static str,
    pub value: IpcValue,
}

impl IpcArg {
    pub fn string(name: &'static str, value: &'static str) -> Self {
        Self {
            name,
            value: IpcValue::String(value),
        }
    }

    pub fn bool(name: &'static str, value: bool) -> Self {
        Self {
            name,
            value: IpcValue::Bool(value),
        }
    }

    pub fn usize(name: &'static str, value: usize) -> Self {
        Self {
            name,
            value: IpcValue::Usize(value),
        }
    }

    pub fn u32(name: &'static str, value: u32) -> Self {
        Self {
            name,
            value: IpcValue::U32(value),
        }
    }

    pub fn f32(name: &'static str, value: f32) -> Self {
        Self {
            name,
            value: IpcValue::F32(value),
        }
    }

    pub fn f64(name: &'static str, value: f64) -> Self {
        Self {
            name,
            value: IpcValue::F64(value),
        }
    }

    pub fn runtime(name: &'static str, value: RuntimeValueRef) -> Self {
        Self {
            name,
            value: IpcValue::Runtime(value),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum IpcValue {
    String(&'static str),
    Bool(bool),
    Usize(usize),
    U32(u32),
    F32(f32),
    F64(f64),
    Runtime(RuntimeValueRef),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeValueRef {
    ThemeJson,
    HighlightRules,
    HighlightPerf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IpcRuntimeInputs {
    pub theme_json: Value,
    pub highlight_rules: Value,
    pub highlight_perf: Value,
}

impl IpcRuntimeInputs {
    pub fn placeholder() -> Self {
        Self {
            theme_json: Value::String("<themeJson>".to_string()),
            highlight_rules: Value::String("<highlightRules>".to_string()),
            highlight_perf: Value::String("<highlightPerf>".to_string()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedIpcBatch {
    pub calls: Vec<ResolvedIpcCall>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedIpcCall {
    pub command: &'static str,
    pub args: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IpcDispatchReport {
    pub invoked: Vec<&'static str>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IpcDispatchError {
    pub command: &'static str,
    pub message: String,
}

pub trait IpcInvoker {
    fn invoke(&mut self, call: &ResolvedIpcCall) -> Result<(), String>;
}

pub fn batch(plan: &NativeAdapterPlan, config: &NativeAdapterConfig) -> IpcBatch {
    let mut calls = Vec::new();
    calls.extend(
        plan.lifecycle
            .iter()
            .map(|command| lifecycle_call(*command)),
    );
    calls.extend(
        plan.renderer
            .iter()
            .map(|command| renderer_call(*command, config)),
    );
    IpcBatch { calls }
}

pub fn resolve_batch(
    batch: &IpcBatch,
    runtime: &IpcRuntimeInputs,
) -> Result<ResolvedIpcBatch, IpcDispatchError> {
    batch
        .calls
        .iter()
        .map(|call| resolve_call(call, runtime))
        .collect::<Result<Vec<_>, _>>()
        .map(|calls| ResolvedIpcBatch { calls })
}

pub fn dispatch_batch(
    batch: &IpcBatch,
    runtime: &IpcRuntimeInputs,
    invoker: &mut impl IpcInvoker,
) -> Result<IpcDispatchReport, IpcDispatchError> {
    dispatch_resolved_batch(&resolve_batch(batch, runtime)?, invoker)
}

pub fn dispatch_resolved_batch(
    batch: &ResolvedIpcBatch,
    invoker: &mut impl IpcInvoker,
) -> Result<IpcDispatchReport, IpcDispatchError> {
    let mut invoked = Vec::new();

    for call in &batch.calls {
        invoked.push(call.command);
        invoker.invoke(call).map_err(|message| IpcDispatchError {
            command: call.command,
            message,
        })?;
    }

    Ok(IpcDispatchReport { invoked })
}

fn resolve_call(
    call: &IpcCall,
    runtime: &IpcRuntimeInputs,
) -> Result<ResolvedIpcCall, IpcDispatchError> {
    let mut args = Map::new();
    for arg in &call.args {
        args.insert(
            arg.name.to_string(),
            resolve_value(&arg.value, runtime, call.command)?,
        );
    }

    Ok(ResolvedIpcCall {
        command: call.command,
        args: Value::Object(args),
    })
}

fn resolve_value(
    value: &IpcValue,
    runtime: &IpcRuntimeInputs,
    command: &'static str,
) -> Result<Value, IpcDispatchError> {
    match value {
        IpcValue::String(value) => Ok(Value::String((*value).to_string())),
        IpcValue::Bool(value) => Ok(Value::Bool(*value)),
        IpcValue::Usize(value) => Ok(Value::Number(Number::from(*value as u64))),
        IpcValue::U32(value) => Ok(Value::Number(Number::from(*value))),
        IpcValue::F32(value) => number(*value as f64, command),
        IpcValue::F64(value) => number(*value, command),
        IpcValue::Runtime(RuntimeValueRef::ThemeJson) => Ok(runtime.theme_json.clone()),
        IpcValue::Runtime(RuntimeValueRef::HighlightRules) => Ok(runtime.highlight_rules.clone()),
        IpcValue::Runtime(RuntimeValueRef::HighlightPerf) => Ok(runtime.highlight_perf.clone()),
    }
}

fn number(value: f64, command: &'static str) -> Result<Value, IpcDispatchError> {
    Number::from_f64(value)
        .map(Value::Number)
        .ok_or_else(|| IpcDispatchError {
            command,
            message: format!("non-finite numeric IPC argument: {value}"),
        })
}

fn lifecycle_call(command: TerminalLifecycleCommand) -> IpcCall {
    match command {
        TerminalLifecycleCommand::UpdateTheme { session_id } => IpcCall::new(
            "terminal_update_theme",
            vec![
                IpcArg::string("sessionId", session_id),
                IpcArg::runtime("themeJson", RuntimeValueRef::ThemeJson),
            ],
        ),
        TerminalLifecycleCommand::Create {
            session_id,
            cols,
            rows,
            cell_width,
            cell_height,
            scrollback_lines,
            is_local,
        } => IpcCall::new(
            "terminal_create",
            vec![
                IpcArg::string("sessionId", session_id),
                IpcArg::usize("cols", cols),
                IpcArg::usize("rows", rows),
                IpcArg::f32("cellWidth", cell_width),
                IpcArg::f32("cellHeight", cell_height),
                IpcArg::usize("scrollback", scrollback_lines),
                IpcArg::bool("isLocal", is_local),
            ],
        ),
        TerminalLifecycleCommand::SetCursorStyle { session_id, style } => IpcCall::new(
            "terminal_set_cursor_style",
            vec![
                IpcArg::string("sessionId", session_id),
                IpcArg::string("style", style),
            ],
        ),
        TerminalLifecycleCommand::UpdateHighlightRules { session_id } => IpcCall::new(
            "terminal_update_highlight_rules",
            vec![
                IpcArg::string("sessionId", session_id),
                IpcArg::runtime("rules", RuntimeValueRef::HighlightRules),
                IpcArg::runtime("perf", RuntimeValueRef::HighlightPerf),
            ],
        ),
        TerminalLifecycleCommand::UpdateFont { session_id, font } => IpcCall::new(
            "terminal_update_font",
            vec![
                IpcArg::string("sessionId", session_id),
                IpcArg::string("family", font.family),
                IpcArg::f32("size", font.size),
                IpcArg::f32("letterSpacing", font.letter_spacing),
                IpcArg::f32("lineHeight", font.line_height),
                IpcArg::f64("dpr", font.dpr),
            ],
        ),
        TerminalLifecycleCommand::Resize {
            session_id,
            cols,
            rows,
            cell_width,
            cell_height,
        } => IpcCall::new(
            "terminal_resize",
            vec![
                IpcArg::string("sessionId", session_id),
                IpcArg::usize("cols", cols),
                IpcArg::usize("rows", rows),
                IpcArg::f32("cellWidth", cell_width),
                IpcArg::f32("cellHeight", cell_height),
            ],
        ),
        TerminalLifecycleCommand::ResizeSurface {
            session_id,
            surface,
        } => IpcCall::new(
            "terminal_resize_surface",
            vec![
                IpcArg::string("sessionId", session_id),
                IpcArg::u32("width", surface.width),
                IpcArg::u32("height", surface.height),
            ],
        ),
    }
}

fn renderer_call(command: RendererIpcCommand, config: &NativeAdapterConfig) -> IpcCall {
    match command {
        RendererIpcCommand::StartRender { session_id } => IpcCall::new(
            "terminal_start_render",
            vec![
                IpcArg::string("sessionId", session_id),
                IpcArg::string("fontFamily", config.font.family),
                IpcArg::f32("fontSize", config.font.size),
                IpcArg::f32("letterSpacing", config.font.letter_spacing),
                IpcArg::f32("lineHeight", config.font.line_height),
                IpcArg::f64("dpr", config.font.dpr),
            ],
        ),
        RendererIpcCommand::StopRender { session_id } => IpcCall::new(
            "terminal_stop_render",
            vec![IpcArg::string("sessionId", session_id)],
        ),
        RendererIpcCommand::SetViewport { session_id, rect } => IpcCall::new(
            "terminal_set_viewport",
            vec![
                IpcArg::string("sessionId", session_id),
                IpcArg::f32("x", rect.x as f32),
                IpcArg::f32("y", rect.y as f32),
                IpcArg::f32("width", rect.width as f32),
                IpcArg::f32("height", rect.height as f32),
            ],
        ),
        RendererIpcCommand::SetFocused {
            session_id,
            focused,
        } => IpcCall::new(
            "terminal_set_focused",
            vec![
                IpcArg::string("sessionId", session_id),
                IpcArg::bool("focused", focused),
            ],
        ),
    }
}
