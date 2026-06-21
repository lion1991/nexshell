use crate::ipc_dispatcher::{
    self, IpcDispatchError, IpcRuntimeInputs, ResolvedIpcBatch, ResolvedIpcCall,
};
use crate::native_shell_host::NativeShellFrame;
use serde_json::{json, Value};

pub fn resolve_frame(
    frame: &NativeShellFrame,
    runtime: &IpcRuntimeInputs,
) -> Result<Value, IpcDispatchError> {
    let batch = ipc_dispatcher::resolve_batch(&frame.ipc, runtime)?;

    Ok(frontend_frame(batch))
}

fn frontend_frame(batch: ResolvedIpcBatch) -> Value {
    let calls = batch
        .calls
        .into_iter()
        .map(frontend_call)
        .collect::<Vec<_>>();

    json!({
        "ipc": {
            "calls": calls,
        },
    })
}

fn frontend_call(call: ResolvedIpcCall) -> Value {
    json!({
        "command": call.command,
        "args": call.args,
    })
}
