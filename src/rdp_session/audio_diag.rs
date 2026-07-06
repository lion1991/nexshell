use ironrdp_dvc::{DrdynvcClient, DvcChannelListener, DvcProcessor, DynamicChannelId};
use ironrdp_rdpsnd::client::Rdpsnd;
use ironrdp_svc::StaticChannelSet;

const DIAG_ENV: &str = "NEXSHELL_RDP_AUDIO_DIAG";
const AUDIO_PLAYBACK_DVC: &str = "AUDIO_PLAYBACK_DVC";
const AUDIO_PLAYBACK_LOSSY_DVC: &str = "AUDIO_PLAYBACK_LOSSY_DVC";

pub(super) fn enabled() -> bool {
    std::env::var_os(DIAG_ENV).is_some()
}

pub(super) fn static_channel_names(channels: &StaticChannelSet) -> Vec<String> {
    channels
        .values()
        .map(|channel| {
            channel
                .channel_name()
                .as_str()
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("{:?}", channel.channel_name()))
        })
        .collect()
}

pub(super) fn rdpsnd_channel_id(channels: &StaticChannelSet) -> Option<u16> {
    channels.get_channel_id_by_type::<Rdpsnd>()
}

pub(super) fn log_advertised_static_channels(channels: &StaticChannelSet) {
    if !enabled() {
        return;
    }

    eprintln!(
        "[rdp-audio] advertised static channels: {}",
        static_channel_names(channels).join(", ")
    );
}

pub(super) fn log_negotiated_rdpsnd_channel(channels: &StaticChannelSet) {
    if !enabled() {
        return;
    }

    match rdpsnd_channel_id(channels) {
        Some(channel_id) => {
            eprintln!("[rdp-audio] legacy rdpsnd negotiated static channel_id={channel_id}")
        }
        None => eprintln!("[rdp-audio] legacy rdpsnd has no negotiated static channel_id"),
    }
}

pub(super) fn attach_audio_dvc_probes(client: DrdynvcClient) -> DrdynvcClient {
    if !enabled() {
        return client;
    }

    eprintln!(
        "[rdp-audio] diag enabled; probing unsupported Windows App audio DVCs: {AUDIO_PLAYBACK_DVC}, {AUDIO_PLAYBACK_LOSSY_DVC}"
    );

    client
        .with_listener(AudioPlaybackDvcProbe::new(AUDIO_PLAYBACK_DVC))
        .with_listener(AudioPlaybackDvcProbe::new(AUDIO_PLAYBACK_LOSSY_DVC))
}

#[derive(Debug)]
pub(super) struct AudioPlaybackDvcProbe {
    name: &'static str,
}

impl AudioPlaybackDvcProbe {
    pub(super) fn new(name: &'static str) -> Self {
        Self { name }
    }
}

impl DvcChannelListener for AudioPlaybackDvcProbe {
    fn channel_name(&self) -> &str {
        self.name
    }

    fn create(&mut self, channel_id: DynamicChannelId) -> Option<Box<dyn DvcProcessor>> {
        eprintln!(
            "[rdp-audio] server requested unsupported audio DVC '{}' channel_id={channel_id}; nexshell currently only implements legacy RDPSND",
            self.name
        );
        None
    }
}

#[cfg(test)]
mod tests {
    use core::any::TypeId;

    use ironrdp_dvc::DvcChannelListener as _;
    use ironrdp_rdpsnd::client::{NoopRdpsndBackend, Rdpsnd};
    use ironrdp_svc::{StaticChannelSet, SvcProcessor as _};

    use super::{rdpsnd_channel_id, AudioPlaybackDvcProbe};

    #[test]
    fn audio_playback_dvc_probe_reports_channel_name() {
        let probe = AudioPlaybackDvcProbe::new("AUDIO_PLAYBACK_DVC");

        assert_eq!(probe.channel_name(), "AUDIO_PLAYBACK_DVC");
    }

    #[test]
    fn audio_playback_dvc_probe_rejects_channel_creation() {
        let mut probe = AudioPlaybackDvcProbe::new("AUDIO_PLAYBACK_LOSSY_DVC");

        assert!(probe.create(7).is_none());
    }

    #[test]
    fn rdpsnd_channel_id_returns_attached_static_channel_id() {
        let mut channels = StaticChannelSet::new();
        let rdpsnd = Rdpsnd::new(Box::new(NoopRdpsndBackend));
        assert_eq!(rdpsnd.channel_name(), Rdpsnd::NAME);
        channels.insert(rdpsnd);
        channels.attach_channel_id(TypeId::of::<Rdpsnd>(), 1007);

        assert_eq!(rdpsnd_channel_id(&channels), Some(1007));
    }
}
