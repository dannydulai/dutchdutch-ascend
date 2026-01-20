use crate::error::{AscendError, Result};
use crate::protocol::{Method, Request, TargetType};
use crate::speaker_connection::SpeakerConnection;
use crate::types::{ChannelMapping, DeviceId, GainData, GainValue, MuteData, MuteState, Preset, RoomId, ToneSettings, VoicingProfile};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Interface for controlling a room
///
/// A `Room` provides high-level methods for controlling speaker systems,
/// including volume, mute, voicing profiles, presets, and channel mapping.
#[derive(Clone)]
pub struct Room {
    speaker: Arc<SpeakerConnection>,
    state: Arc<Mutex<RoomState>>,
}

/// Room state snapshot
#[derive(Clone)]
pub struct RoomState {
    // Core identity
    pub id: RoomId,
    pub name: String,

    // Members is an object mapping device IDs to position IDs
    pub members: BTreeMap<DeviceId, String>,

    // Gain data with global value and limits
    pub gain: GainData,

    // Mute data with global and per-position values
    pub mute: MuteData,

    // Standby/sleep state
    pub sleep: bool,

    // Selected input source
    pub selected_input: Option<String>,

    // Selected XLR mode
    pub selected_xlr: Option<String>,

    // Raw input modes from JSON (contains all modes including XLR)
    pub input_modes_raw: Vec<String>,

    // Available input modes (excluding XLR modes) - computed from input_modes_raw
    pub input_modes: Vec<String>,

    // Available XLR input modes (aes, analogLowGain, analogHighGain) - computed from input_modes_raw
    pub xlr_input_modes: Vec<String>,

    // Selected voicing profile ID
    pub selected_voicing_profile: Option<String>,

    // Available voicing profiles (map of ID to profile)
    pub voicing: BTreeMap<String, VoicingProfile>,

    // Available presets (map of ID to preset)
    pub presets: BTreeMap<String, Preset>,

    // Last selected preset ID
    pub last_selected_preset: Option<String>,

    // Channel mapping configuration
    pub channel_mapping: Option<ChannelMapping>,

    // Streaming state
    pub streaming: Option<bool>,

    // Linear phase filter setting
    pub linear_phase: bool,

    // Raw JSON copy
    pub raw_json: serde_json::Value,
}

impl Room {
    /// Create a new Room instance from raw JSON
    pub(crate) fn new(speaker: Arc<SpeakerConnection>, json: serde_json::Value) -> Result<Self> {
        let state = parse_room_state_from_json(json)?;
        Ok(Self {
            speaker,
            state: Arc::new(Mutex::new(state)),
        })
    }

    /// Get the room ID
    pub fn id(&self) -> uuid::Uuid {
        self.state.lock().unwrap().id
    }

    /// Get the room name
    pub fn name(&self) -> String {
        self.state.lock().unwrap().name.clone()
    }

    /// Get the raw JSON representation of the room state
    pub fn raw_json(&self) -> serde_json::Value {
        self.state.lock().unwrap().raw_json.clone()
    }

    /// Get a snapshot of the complete room state for rendering
    /// This ensures consistent values across a single render frame
    pub fn state_snapshot(&self) -> RoomState {
        self.state.lock().unwrap().clone()
    }

    /// Get the gain data including global value, limits, and positional gains
    pub fn gain(&self) -> GainData {
        self.state.lock().unwrap().gain.clone()
    }

    /// Get the mute data including global and per-position mute states
    pub fn mute(&self) -> MuteData {
        self.state.lock().unwrap().mute.clone()
    }

    /// Get the standby/sleep state
    pub fn sleep(&self) -> bool {
        self.state.lock().unwrap().sleep
    }

    /// Get the selected input
    pub fn selected_input(&self) -> Option<String> {
        self.state.lock().unwrap().selected_input.clone()
    }

    /// Get the selected XLR mode
    pub fn selected_xlr(&self) -> Option<String> {
        self.state.lock().unwrap().selected_xlr.clone()
    }

    /// Get the available input modes
    pub fn input_modes(&self) -> Vec<String> {
        self.state.lock().unwrap().input_modes.clone()
    }

    /// Get the available XLR input modes
    pub fn xlr_input_modes(&self) -> Vec<String> {
        self.state.lock().unwrap().xlr_input_modes.clone()
    }

    /// Get the linear phase state
    pub fn linear_phase(&self) -> bool {
        self.state.lock().unwrap().linear_phase
    }

    /// Get the number of member devices
    pub fn member_count(&self) -> usize {
        self.state.lock().unwrap().members.len()
    }

    /// Get the voicing profiles
    pub fn voicing_profiles(&self) -> BTreeMap<String, VoicingProfile> {
        self.state.lock().unwrap().voicing.clone()
    }

    /// Get the selected voicing profile ID
    pub fn selected_voicing_profile(&self) -> Option<String> {
        self.state.lock().unwrap().selected_voicing_profile.clone()
    }

    /// Get the presets
    pub fn presets(&self) -> BTreeMap<String, Preset> {
        self.state.lock().unwrap().presets.clone()
    }

    /// Get the last selected preset ID
    pub fn last_selected_preset(&self) -> Option<String> {
        self.state.lock().unwrap().last_selected_preset.clone()
    }

    /// Update the room state from raw JSON (called internally by Discovery when state updates arrive)
    pub(crate) fn update_from_json(&self, json: serde_json::Value) -> Result<()> {
        let new_state = parse_room_state_from_json(json)?;
        *self.state.lock().unwrap() = new_state;
        Ok(())
    }

    /// Refresh the room state from the speaker
    pub async fn refresh(&mut self) -> Result<()> {
        let request = Request::new("network", Method::Read);
        let response = self.speaker.connection().send_request(request).await?;

        let data = response
            .data
            .ok_or_else(|| AscendError::InvalidResponse("No data in network response".to_string()))?;

        // Parse the state to find our room
        let state = data
            .get("state")
            .ok_or_else(|| AscendError::InvalidResponse("No state in network response".to_string()))?;

        let state_obj = state
            .as_object()
            .ok_or_else(|| AscendError::InvalidResponse("State is not an object".to_string()))?;

        // Find our room by ID
        let current_id = self.state.lock().unwrap().id;
        for (_state_id, state_entry) in state_obj {
            if let Some(entry_data) = state_entry.get("data") {
                if entry_data.get("type").and_then(|v| v.as_str()) == Some("room") {
                    if let Some(id_str) = entry_data.get("id").and_then(|v| v.as_str()) {
                        if let Ok(id) = uuid::Uuid::parse_str(id_str) {
                            if id == current_id {
                                self.update_from_json(entry_data.clone())?;
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }

        Err(AscendError::RoomNotFound(current_id.to_string()))
    }

    // ========== Volume Control ==========

    /// Set the global room volume in dB
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use dutchdutch_ascend::AscendClient;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = AscendClient::connect("192.168.1.100", 8768).await?;
    /// let room = client.room().await?;
    /// room.set_gain(-20.0).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_gain(&self, gain: GainValue) -> Result<()> {
        let request = Request::new("gain2", Method::Update)
            .with_target(TargetType::Room, self.state.lock().unwrap().id.to_string())
            .with_data(json!({ "gain": gain }));

        self.speaker.connection().send_request(request).await?;
        Ok(())
    }

    // ========== Mute Control ==========

    /// Set the global room mute state
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use dutchdutch_ascend::AscendClient;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = AscendClient::connect("192.168.1.100", 8768).await?;
    /// let room = client.room().await?;
    /// room.set_mute(true).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_mute(&self, mute: MuteState) -> Result<()> {
        let request = Request::new("mute", Method::Update)
            .with_target(TargetType::Room, self.state.lock().unwrap().id.to_string())
            .with_data(json!([{
                "mute": mute,
                "positionID": "global"
            }]));

        self.speaker.connection().send_request(request).await?;
        Ok(())
    }

    // ========== Standby/Power Control ==========

    /// Set the standby/sleep state
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use dutchdutch_ascend::AscendClient;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = AscendClient::connect("192.168.1.100", 8768).await?;
    /// let room = client.room().await?;
    /// room.set_standby(true).await?; // Put room into standby
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_standby(&self, standby: bool) -> Result<()> {
        let request = Request::new("sleep", Method::Update)
            .with_target(TargetType::Room, self.state.lock().unwrap().id.to_string())
            .with_data(json!({ "enable": standby }));

        self.speaker.connection().send_request(request).await?;
        Ok(())
    }

    // ========== Input Selection ==========

    /// Set the selected input source
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use dutchdutch_ascend::AscendClient;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = AscendClient::connect("192.168.1.100", 8768).await?;
    /// let room = client.room().await?;
    /// room.set_input("XLR").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_input(&self, input: impl Into<String>) -> Result<()> {
        let request = Request::new("selectedInput", Method::Update)
            .with_target(TargetType::Room, self.state.lock().unwrap().id.to_string())
            .with_data(json!({ "input": input.into() }));

        self.speaker.connection().send_request(request).await?;
        Ok(())
    }

    /// Set the selected XLR input
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use dutchdutch_ascend::AscendClient;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = AscendClient::connect("192.168.1.100", 8768).await?;
    /// let room = client.room().await?;
    /// room.set_xlr_input("aes").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_xlr_mode(&self, mode: impl Into<String>) -> Result<()> {
        let request = Request::new("selectedXLR", Method::Update)
            .with_target(TargetType::Room, self.state.lock().unwrap().id.to_string())
            .with_data(json!({ "xlr": mode.into() }));

        self.speaker.connection().send_request(request).await?;
        Ok(())
    }

    /// Set the linear phase filter state
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use dutchdutch_ascend::AscendClient;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = AscendClient::connect("192.168.1.100", 8768).await?;
    /// let room = client.room().await?;
    /// room.set_linear_phase(true).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_linear_phase(&self, enabled: bool) -> Result<()> {
        let request = Request::new("linear-phase", Method::Update)
            .with_target(TargetType::Room, self.state.lock().unwrap().id.to_string())
            .with_data(json!({ "enable": enabled }));

        self.speaker.connection().send_request(request).await?;
        Ok(())
    }

    /// Select a voicing profile
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use dutchdutch_ascend::AscendClient;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = AscendClient::connect("192.168.1.100", 8768).await?;
    /// let room = client.room().await?;
    /// room.select_voicing("Neutral").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn select_voicing(&self, profile: impl Into<String>) -> Result<()> {
        let request = Request::new("tone-control", Method::Select)
            .with_target(TargetType::Room, self.state.lock().unwrap().id.to_string())
            .with_data(json!({ "voicing": profile.into() }));

        self.speaker.connection().send_request(request).await?;
        Ok(())
    }

    /// Update tone control settings (sub, mid, treble)
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use dutchdutch_ascend::{AscendClient, ToneSettings};
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = AscendClient::connect("192.168.1.100", 8768).await?;
    /// let room = client.room().await?;
    /// room.update_tone(ToneSettings {
    ///     sub: 2.0,
    ///     mid: 0.0,
    ///     treble: -1.0,
    /// }).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn update_tone(&self, tone: ToneSettings) -> Result<()> {
        let request = Request::new("tone-control", Method::Update)
            .with_target(TargetType::Room, self.state.lock().unwrap().id.to_string())
            .with_data(serde_json::to_value(&tone)?);

        self.speaker.connection().send_request(request).await?;
        Ok(())
    }

    /// Select and apply a preset
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use dutchdutch_ascend::AscendClient;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = AscendClient::connect("192.168.1.100", 8768).await?;
    /// let room = client.room().await?;
    /// room.select_preset("my-preset").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn select_preset(&self, preset_id: impl Into<String>) -> Result<()> {
        let request = Request::new("preset2", Method::Select)
            .with_target(TargetType::Room, self.state.lock().unwrap().id.to_string())
            .with_data(json!({ "id": preset_id.into() }));

        self.speaker.connection().send_request(request).await?;
        Ok(())
    }
}

/// Parse room state from JSON value
fn parse_room_state_from_json(json: serde_json::Value) -> Result<RoomState> {
    // API bug workaround: Replace "AES Streamer" with "XLR"
    let mut json = json;
    if let Some(obj) = json.as_object_mut() {
        if let Some(input_modes) = obj.get_mut("inputModes").and_then(|v| v.as_array_mut()) {
            for mode in input_modes.iter_mut() {
                if mode.as_str() == Some("AES Streamer") {
                    *mode = serde_json::Value::String("XLR".to_string());
                }
            }
        }
        if let Some(selected) = obj.get_mut("selectedInput").and_then(|v| v.as_str()) {
            if selected == "AES Streamer" {
                obj.insert("selectedInput".to_string(), serde_json::Value::String("XLR".to_string()));
            }
        }
    }

    let id: RoomId = json.get("id")
        .and_then(|v| v.as_str())
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .ok_or_else(|| AscendError::InvalidResponse("Missing or invalid room id".to_string()))?;

    let name: String = json.get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AscendError::InvalidResponse("Missing room name".to_string()))?;

    let members: BTreeMap<DeviceId, String> = json.get("members")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let gain: GainData = json.get("gain")
        .ok_or_else(|| AscendError::InvalidResponse("Missing gain data".to_string()))
        .and_then(|v| serde_json::from_value(v.clone()).map_err(AscendError::Json))?;

    let mute: MuteData = json.get("mute")
        .ok_or_else(|| AscendError::InvalidResponse("Missing mute data".to_string()))
        .and_then(|v| serde_json::from_value(v.clone()).map_err(AscendError::Json))?;

    let sleep: bool = json.get("sleep")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let selected_input: Option<String> = json.get("selectedInput")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let selected_xlr: Option<String> = json.get("selectedXLR")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let input_modes_raw: Vec<String> = json.get("inputModes")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // Split input modes into regular and XLR
    let xlr_mode_names = ["aes", "analogLowGain", "analogHighGain"];
    let mut input_modes = Vec::new();
    let mut xlr_input_modes = Vec::new();

    for mode in &input_modes_raw {
        if xlr_mode_names.contains(&mode.as_str()) {
            xlr_input_modes.push(mode.clone());
        } else {
            input_modes.push(mode.clone());
        }
    }

    let selected_voicing_profile: Option<String> = json.get("selectedVoicingProfile")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let voicing: BTreeMap<String, VoicingProfile> = json.get("voicing")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let presets: BTreeMap<String, Preset> = json.get("presets")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let last_selected_preset: Option<String> = json.get("lastSelectedPreset")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let channel_mapping: Option<ChannelMapping> = json.get("channelMapping")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let streaming: Option<bool> = json.get("streaming")
        .and_then(|v| v.as_bool());

    let linear_phase: bool = json.get("linearPhase")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Ok(RoomState {
        id,
        name,
        members,
        gain,
        mute,
        sleep,
        selected_input,
        selected_xlr,
        input_modes_raw,
        input_modes,
        xlr_input_modes,
        selected_voicing_profile,
        voicing,
        presets,
        last_selected_preset,
        channel_mapping,
        streaming,
        linear_phase,
        raw_json: json,
    })
}
