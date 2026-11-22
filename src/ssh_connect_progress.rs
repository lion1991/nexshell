//! SSH 连接过渡画面的状态：连接中由 UI 在终端上盖一层原生动画（进度环 + 阶段），
//! 需要用户输入（密码 / 主机密钥确认）或失败时交还终端，写回原来的文字横幅。

use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SshConnectStage {
    Tcp,
    Auth,
    Shell,
}

impl SshConnectStage {
    pub const ALL: [SshConnectStage; 3] = [Self::Tcp, Self::Auth, Self::Shell];

    pub fn index(self) -> usize {
        match self {
            Self::Tcp => 0,
            Self::Auth => 1,
            Self::Shell => 2,
        }
    }

    fn banner_step(self) -> &'static str {
        match self {
            Self::Tcp => "正在建立 TCP 连接",
            Self::Auth => "正在进行身份验证",
            Self::Shell => "正在打开远程终端",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SshConnectProgress {
    pub stage: SshConnectStage,
    /// `user@host:port`。
    pub target: String,
    pub started_at: Instant,
    pub stage_started_at: Instant,
}

impl SshConnectProgress {
    pub fn new(target: String, now: Instant) -> Self {
        Self {
            stage: SshConnectStage::Tcp,
            target,
            started_at: now,
            stage_started_at: now,
        }
    }

    /// 进入新阶段；同一阶段重复调用（如主机密钥确认后重连）不重置阶段计时。
    pub fn advance(&mut self, stage: SshConnectStage, now: Instant) {
        if self.stage != stage {
            self.stage = stage;
            self.stage_started_at = now;
        }
    }
}

/// 交还终端时写进 grid 的文字横幅（清屏、隐藏光标）。
pub fn ssh_connect_banner(stage: SshConnectStage, target: &str) -> String {
    ssh_connect_banner_text(stage.banner_step(), target)
}

/// 同上，步骤文字自定（如主机密钥确认后的「正在重新连接」）。
pub fn ssh_connect_banner_text(step: &str, target: &str) -> String {
    let mut buf = String::from("\x1b[2J\x1b[3J\x1b[H\x1b[?25l\r\n");
    buf.push_str("  \x1b[1;36m⟐ NexShell SSH\x1b[0m\r\n\r\n");
    buf.push_str(&format!("  \x1b[37m  {target}\x1b[0m\r\n\r\n"));
    buf.push_str(&format!("  \x1b[33m◌ {step}...\x1b[0m\r\n"));
    buf
}

/// 进度条比例：已完成阶段各占 1/3，当前阶段按停留时间渐近填到该段的 90%，不会自己走满。
pub fn ssh_connect_fraction(progress: &SshConnectProgress, now: Instant) -> f32 {
    let in_stage = now
        .saturating_duration_since(progress.stage_started_at)
        .as_secs_f32();
    let creep = 0.9 * (1.0 - (-in_stage / 1.2).exp());
    (progress.stage.index() as f32 + creep) / SshConnectStage::ALL.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn advance_keeps_stage_timer_for_same_stage() {
        let t0 = Instant::now();
        let mut progress = SshConnectProgress::new("root@h:22".into(), t0);
        let t1 = t0 + Duration::from_secs(1);
        progress.advance(SshConnectStage::Tcp, t1);
        assert_eq!(progress.stage_started_at, t0);
        progress.advance(SshConnectStage::Auth, t1);
        assert_eq!(progress.stage, SshConnectStage::Auth);
        assert_eq!(progress.stage_started_at, t1);
        assert_eq!(progress.started_at, t0);
    }

    #[test]
    fn fraction_stays_inside_current_stage_segment() {
        let t0 = Instant::now();
        let mut progress = SshConnectProgress::new("root@h:22".into(), t0);
        assert_eq!(ssh_connect_fraction(&progress, t0), 0.0);
        let late = t0 + Duration::from_secs(60);
        let tcp_late = ssh_connect_fraction(&progress, late);
        assert!(tcp_late < 1.0 / 3.0 && tcp_late > 0.29);
        progress.advance(SshConnectStage::Shell, late);
        let shell = ssh_connect_fraction(&progress, late + Duration::from_secs(60));
        assert!(shell < 1.0 && shell > 0.96);
    }

    #[test]
    fn banner_contains_target_and_step() {
        let banner = ssh_connect_banner(SshConnectStage::Auth, "root@h:22");
        assert!(banner.contains("root@h:22"));
        assert!(banner.contains("正在进行身份验证"));
    }
}
