use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Room identifier
pub type RoomId = Uuid;

/// Device identifier
pub type DeviceId = String;

/// Position identifier (speaker position in room)
pub type PositionId = String;

/// Gain value in decibels
pub type GainValue = f64;

/// Mute state
pub type MuteState = bool;

// RoomDocument is now merged into Room struct - this type is kept for backward compatibility
// but not used internally anymore

/// Device information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub name: String,

    /// Product tags (e.g., "8c", "subwoofer")
    #[serde(default)]
    pub tags: Vec<String>,

    /// Licensed features
    #[serde(default)]
    pub licenses: Vec<String>,
}

/// Gain data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GainData {
    /// Global gain value in dB
    pub global: f64,

    /// Gain limits
    #[serde(default)]
    pub limits: GainLimits,
}

impl GainData {
    /// Get the minimum allowed gain value
    pub fn min(&self) -> f64 {
        self.limits.min
    }

    /// Get the maximum allowed gain value
    pub fn max(&self) -> f64 {
        self.limits.max
    }

    /// Get the gain adjustment step size
    pub fn step(&self) -> f64 {
        self.limits.step
    }
}

/// Gain limits
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GainLimits {
    #[serde(default = "default_min")]
    pub min: f64,
    #[serde(default)]
    pub max: f64,
    #[serde(default = "default_step")]
    pub step: f64,
}

fn default_min() -> f64 {
    -80.0
}

fn default_step() -> f64 {
    0.5
}

/// Mute data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MuteData {
    /// Global mute state
    pub global: bool,

    /// Per-position mute states
    #[serde(flatten)]
    pub positions: BTreeMap<String, bool>,
}

impl MuteData {
    /// Get the mute state for a specific position
    pub fn position(&self, position_id: &str) -> Option<bool> {
        self.positions.get(position_id).copied()
    }

    /// Get all position IDs that have mute state
    pub fn position_ids(&self) -> Vec<String> {
        self.positions.keys().cloned().collect()
    }

    /// Check if any position is muted (regardless of global state)
    pub fn any_position_muted(&self) -> bool {
        self.positions.values().any(|&muted| muted)
    }
}

/// Voicing profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoicingProfile {
    pub name: String,
    pub sub: f64,
    pub bass: f64,
    pub treble: f64,
    #[serde(default)]
    #[serde(rename = "paramEQ")]
    pub param_eq: BTreeMap<String, serde_json::Value>,
}

/// Tone control settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToneSettings {
    /// Subwoofer gain adjustment
    pub sub: f64,

    /// Midrange gain adjustment
    pub mid: f64,

    /// Treble gain adjustment
    pub treble: f64,
}

/// Preset configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    #[serde(default)]
    pub description: String,

    /// Preset settings
    #[serde(default)]
    pub settings: BTreeMap<String, serde_json::Value>,

    /// Whether this is a read-only preset
    #[serde(default)]
    pub readonly: bool,
}

/// Channel mapping configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMapping {
    /// Mapping from input channels to output gains
    #[serde(flatten)]
    pub channels: BTreeMap<String, ChannelGains>,
}

/// Gains for left and right channels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelGains {
    pub left: f64,
    pub right: f64,
}


/// Discovered room information from cloud discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredRoom {
    pub id: RoomId,
    pub name: String,
    pub master_ip: String,
    pub master_port: u16,
    pub members: Vec<DeviceId>,
}

impl DiscoveredRoom {
    /// Connect to this discovered room and return the AscendClient
    ///
    /// # Example
    ///
    /// ```no_run
    /// use dutchdutch_ascend::Discovery;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let mut discovery = Discovery::new();
    ///     discovery.start().await?;
    ///
    ///     tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    ///
    ///     if let Some(discovered_room) = discovery.rooms().first() {
    ///         let client = discovered_room.connect().await?;
    ///         let rooms = client.rooms().await?;
    ///         if let Some(room) = rooms.first() {
    ///             room.set_gain(-20.0).await?;
    ///         }
    ///     }
    ///
    ///     discovery.stop().await;
    ///     Ok(())
    /// }
    /// ```
    pub async fn connect(&self) -> crate::error::Result<crate::client::AscendClient> {
        crate::client::AscendClient::connect(&self.master_ip, self.master_port).await
    }
}

/// Target information from the targets endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetsResponse {
    pub rooms: BTreeMap<RoomId, serde_json::Value>,
    pub devices: BTreeMap<DeviceId, Device>,
}
