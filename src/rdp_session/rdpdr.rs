//! RDPDR 设备重定向：驱动器共享（文件互拷）+ 满足 rdpsnd 的通道依赖。
//! 抄 mstsc/FreeRDP：Mac 固定目录 `~/NexShell RDP` 在远端表现为 \\tsclient\NexShell 网盘。
//! 后端用 fork 内建的 NixRdpdrBackend（macOS 完整读写文件系统），见 docs/adr/0007。

use std::path::{Path, PathBuf};

use ironrdp_rdpdr::Rdpdr;

const TRACE_ENV: &str = "NEXSHELL_RDP_RDPDR_TRACE";
/// 共享盘 device_id，固定值；避开 smartcard 常用的 1。
const DRIVE_DEVICE_ID: u32 = 4;
/// 远端可见盘名（\\tsclient\NexShell）。
const DRIVE_NAME: &str = "NexShell";
/// Mac 本地共享目录名（挂在 home 下）。
const SHARED_DIR_NAME: &str = "NexShell RDP";

pub(super) fn enabled() -> bool {
    std::env::var_os(TRACE_ENV).is_some()
}

/// rdpdr 后端决策（纯逻辑，不碰 fs，可单测）。
#[derive(Debug, PartialEq, Eq)]
enum RdpdrPlan {
    /// 真实驱动器重定向（NixRdpdrBackend）。
    Drive,
    /// NoopRdpdrBackend，仅满足 rdpsnd 通道依赖。
    Noop,
    /// 不注册 rdpdr。
    None,
}

/// 只有 drive 开且共享目录就绪才挂真实盘；否则 audio 开挂 Noop，都不需要则不挂。
fn plan(enable_drive: bool, drive_dir_ready: bool, enable_audio: bool) -> RdpdrPlan {
    if enable_drive && drive_dir_ready {
        RdpdrPlan::Drive
    } else if enable_audio {
        RdpdrPlan::Noop
    } else {
        RdpdrPlan::None
    }
}

/// 解析并确保存在固定共享目录，返回绝对路径（不带尾斜杠）。
/// HOME 缺失、路径非绝对或创建失败时返回 None——**绝不**降级到 home/"."/"/"：
/// GUI 从 Finder 启动时 cwd 为 "/"，降级会把整个文件系统暴露给远端 RDP 服务端。
pub(super) fn shared_dir() -> Option<String> {
    let dir = shared_dir_path(&home_dir()?);
    // 安全红线：只接受绝对路径，杜绝相对路径落到 cwd。
    if !dir.is_absolute() {
        return None;
    }
    if let Err(err) = std::fs::create_dir_all(&dir) {
        if enabled() {
            eprintln!(
                "[rdp-rdpdr] create shared dir {} failed: {err}",
                dir.display()
            );
        }
        return None;
    }
    Some(trim_trailing_slash(&dir.to_string_lossy()).to_string())
}

/// 纯拼接：home/NexShell RDP（单测用，不碰真实 fs）。
fn shared_dir_path(home: &Path) -> PathBuf {
    home.join(SHARED_DIR_NAME)
}

/// 取 HOME；缺失或为空都当没有（空串会让拼接退化成相对路径）。
fn home_dir() -> Option<PathBuf> {
    match std::env::var_os("HOME") {
        Some(home) if !home.is_empty() => Some(PathBuf::from(home)),
        _ => None,
    }
}

fn trim_trailing_slash(path: &str) -> &str {
    path.strip_suffix('/').unwrap_or(path)
}

/// 组装 rdpdr 静态通道：
/// - `enable_drive` 且共享目录就绪 → 真实驱动器重定向（NixRdpdrBackend + NexShell 盘）
/// - 否则 `enable_audio` → NoopRdpdrBackend（仅满足 rdpsnd 通道依赖）
/// - 都不需要 → None
pub(super) fn build_channel(enable_drive: bool, enable_audio: bool) -> Option<Rdpdr> {
    // enable_drive 时才解析共享目录；拿不到目录绝不挂真实盘（安全红线，见 shared_dir）。
    let dir = if enable_drive { shared_dir() } else { None };
    if enable_drive && dir.is_none() && enabled() {
        eprintln!("[rdp-rdpdr] shared dir unavailable; drive redirection skipped");
    }

    match plan(enable_drive, dir.is_some(), enable_audio) {
        RdpdrPlan::Drive => {
            let base = dir.expect("plan Drive implies shared dir ready");
            if enabled() {
                eprintln!("[rdp-rdpdr] drive redirection on: {DRIVE_NAME} -> {base}");
            }
            let backend = ironrdp_rdpdr_native::backend::NixRdpdrBackend::new(base);
            Some(
                Rdpdr::new(Box::new(backend), "nexshell".to_string())
                    .with_drives(Some(vec![(DRIVE_DEVICE_ID, DRIVE_NAME.to_string())])),
            )
        }
        RdpdrPlan::Noop => {
            if enabled() {
                eprintln!("[rdp-rdpdr] no-op backend (rdpsnd dependency only)");
            }
            Some(Rdpdr::new(
                Box::new(ironrdp_rdpdr::NoopRdpdrBackend),
                "nexshell".to_string(),
            ))
        }
        RdpdrPlan::None => None,
    }
}

#[cfg(test)]
mod tests {
    use ironrdp_svc::SvcProcessor as _;

    use super::*;

    #[test]
    fn shared_dir_path_joins_under_home() {
        let joined = shared_dir_path(Path::new("/Users/matt"));
        assert_eq!(joined, PathBuf::from("/Users/matt/NexShell RDP"));
        assert_eq!(joined.parent(), Some(Path::new("/Users/matt")));
    }

    #[test]
    fn trim_trailing_slash_strips_single_slash() {
        assert_eq!(trim_trailing_slash("/a/b/"), "/a/b");
        assert_eq!(trim_trailing_slash("/a/b"), "/a/b");
    }

    #[test]
    fn plan_mounts_drive_only_when_drive_dir_ready() {
        assert_eq!(plan(true, true, false), RdpdrPlan::Drive);
        assert_eq!(plan(true, true, true), RdpdrPlan::Drive);
    }

    #[test]
    fn plan_falls_back_to_noop_when_drive_dir_missing() {
        // 安全红线：drive 开但目录拿不到，绝不挂真实盘，只在有音频时挂 Noop。
        assert_eq!(plan(true, false, true), RdpdrPlan::Noop);
        assert_eq!(plan(true, false, false), RdpdrPlan::None);
    }

    #[test]
    fn plan_without_drive_follows_audio() {
        assert_eq!(plan(false, false, true), RdpdrPlan::Noop);
        assert_eq!(plan(false, false, false), RdpdrPlan::None);
    }

    #[test]
    fn audio_only_builds_noop_rdpdr_channel() {
        // enable_drive=false 不碰 fs，安全；应得满足 rdpsnd 依赖的 rdpdr 通道。
        let channel = build_channel(false, true).expect("rdpsnd dependency channel");
        assert_eq!(channel.channel_name(), Rdpdr::NAME);
    }

    #[test]
    fn neither_drive_nor_audio_builds_no_channel() {
        assert!(build_channel(false, false).is_none());
    }
}
