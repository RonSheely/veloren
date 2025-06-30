use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AudioOutput {
    /// Veloren's audio system wont work on some systems,
    /// so you can use this to disable it, and allow the
    /// game to function
    // If this option is disabled, functions in the rodio
    // library MUST NOT be called.
    Off,
    #[serde(other)]
    Automatic,
}

impl AudioOutput {
    pub fn is_enabled(&self) -> bool { !matches!(self, Self::Off) }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct AudioVolume {
    pub volume: f32,
    pub muted: bool,
}

impl AudioVolume {
    pub fn new(volume: f32, muted: bool) -> Self { Self { volume, muted } }

    pub fn get_checked(&self) -> f32 {
        match self.muted {
            true => 0.0,
            false => self.volume,
        }
    }
}

/// `AudioSettings` controls the volume of different audio subsystems and which
/// device is used.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    pub master_volume: AudioVolume,
    #[serde(rename = "inactive_master_volume")]
    pub inactive_master_volume_perc: AudioVolume,
    pub music_volume: AudioVolume,
    pub sfx_volume: AudioVolume,
    pub ambience_volume: AudioVolume,
    pub num_sfx_channels: usize,
    pub num_ui_channels: usize,
    pub music_spacing: f32,
    pub subtitles: bool,
    pub combat_music_enabled: bool,
    /// The size of the sample buffer Kira uses. Increasing this may improve
    /// audio performance at the cost of audio latency.
    pub buffer_size: usize,
    /// Set to None to use the default samplerate determined by the game;
    /// otherwise, use Some(samplerate); the game will attempt to force
    /// samplerate to this.
    pub sample_rate: Option<u32>,

    /// Audio Device that Voxygen will use to play audio.
    pub output: AudioOutput,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            master_volume: AudioVolume::new(0.8, false),
            inactive_master_volume_perc: AudioVolume::new(0.5, false),
            music_volume: AudioVolume::new(0.5, false),
            sfx_volume: AudioVolume::new(0.8, false),
            ambience_volume: AudioVolume::new(0.8, false),
            num_sfx_channels: 64,
            num_ui_channels: 16,
            music_spacing: 1.0,
            subtitles: false,
            output: AudioOutput::Automatic,
            combat_music_enabled: false,
            buffer_size: 512,
            sample_rate: None,
        }
    }
}
