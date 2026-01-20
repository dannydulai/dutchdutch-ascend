use crate::error::{AscendError, Result};
use crate::protocol::{Method, Request};
use crate::room::Room;
use crate::speaker_connection::SpeakerConnection;
use crate::types::RoomId;
use futures_util::{SinkExt, StreamExt};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const DISCOVERY_URL: &str = "wss://api.ascend.audio/";
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const SPEAKER_PORT: u16 = 8768;

/// Discovery manager for Ascend speakers
///
/// Manages the discovery process and maintains a persistent list of discovered rooms.
/// The discovery process runs in the background and automatically reconnects if the
/// connection is lost.
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
///     // Wait a bit for discovery
///     tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
///
///     for room in discovery.rooms() {
///         println!("Found room: {} at {}", room.name, room.master_ip);
///     }
///
///     discovery.stop().await;
///     Ok(())
/// }
/// ```
pub struct Discovery {
    speakers: Arc<Mutex<BTreeMap<String, Arc<SpeakerConnection>>>>,
    rooms: Arc<Mutex<BTreeMap<RoomId, Room>>>,
    update_tx: Arc<broadcast::Sender<RoomId>>,
    stop_tx: Option<broadcast::Sender<()>>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl Discovery {
    /// Create a new Discovery manager
    pub fn new() -> Self {
        let (update_tx, _) = broadcast::channel(100);
        Self {
            speakers: Arc::new(Mutex::new(BTreeMap::new())),
            rooms: Arc::new(Mutex::new(BTreeMap::new())),
            update_tx: Arc::new(update_tx),
            stop_tx: None,
            task_handle: None,
        }
    }

    /// Subscribe to room updates
    ///
    /// Returns a receiver that will receive RoomId whenever a room's state is updated
    pub fn subscribe_updates(&self) -> broadcast::Receiver<RoomId> {
        self.update_tx.subscribe()
    }

    /// Get a snapshot of currently discovered rooms
    pub fn rooms(&self) -> Vec<Room> {
        let rooms = self.rooms.lock().unwrap();
        rooms.values().cloned().collect()
    }

    /// Get the number of discovered rooms
    pub fn room_count(&self) -> usize {
        let rooms = self.rooms.lock().unwrap();
        rooms.len()
    }

    /// Clear the list of discovered rooms
    pub fn clear_rooms(&self) {
        let mut rooms = self.rooms.lock().unwrap();
        rooms.clear();
    }

    /// Start the discovery process
    ///
    /// If discovery is already running, it will be stopped and restarted.
    /// The existing room list is preserved.
    pub async fn start(&mut self) -> Result<()> {
        // Stop existing discovery if running
        self.stop().await;

        let (stop_tx, _) = broadcast::channel(1);
        self.stop_tx = Some(stop_tx.clone());

        let speakers = self.speakers.clone();
        let rooms = self.rooms.clone();
        let update_tx = self.update_tx.clone();

        let handle = tokio::spawn(async move {
            let mut backoff = Duration::from_secs(0);
            let mut stop_rx = stop_tx.subscribe();

            loop {
                tokio::select! {
                    _ = stop_rx.recv() => {
                        tracing::info!("Discovery stopped by user");
                        break;
                    }
                    _ = async {
                        if backoff > Duration::from_secs(0) {
                            tracing::info!("Reconnecting to discovery service in {:?}", backoff);
                            sleep(backoff).await;
                        }

                        let mut stop_rx_inner = stop_tx.subscribe();
                        match run_discovery_once(&speakers, &rooms, &update_tx, &mut stop_rx_inner).await {
                            Ok(_) => {
                                tracing::info!("Discovery scan completed");
                                backoff = Duration::from_secs(0);
                            }
                            Err(e) => {
                                tracing::error!("Discovery error: {}", e);
                                // Exponential backoff: 1s, 2s, 4s, 8s, 16s, 32s, 60s (max)
                                if backoff == Duration::from_secs(0) {
                                    backoff = Duration::from_secs(1);
                                } else {
                                    backoff = (backoff * 2).min(MAX_BACKOFF);
                                }
                            }
                        }
                    } => {}
                }
            }
        });

        self.task_handle = Some(handle);
        Ok(())
    }

    /// Stop the discovery process
    ///
    /// The room list is preserved and can be accessed after stopping.
    /// This will close the websocket connection and abort any pending operations.
    pub async fn stop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.task_handle.take() {
            // Give it a moment to stop gracefully
            let _ = tokio::time::timeout(Duration::from_millis(500), handle).await;
        }
    }
}

impl Default for Discovery {
    fn default() -> Self {
        Self::new()
    }
}


async fn run_discovery_once(
    speakers: &Arc<Mutex<BTreeMap<String, Arc<SpeakerConnection>>>>,
    rooms: &Arc<Mutex<BTreeMap<RoomId, Room>>>,
    update_tx: &Arc<broadcast::Sender<RoomId>>,
    stop_rx: &mut broadcast::Receiver<()>,
) -> Result<()> {
    tracing::info!("Connecting to discovery service: {}", DISCOVERY_URL);

    let (ws_stream, _) = connect_async(DISCOVERY_URL).await?;
    let (mut write, mut read) = ws_stream.split();

    // Send discovery request
    let request = Request::new("discovery:local-ips", Method::Read);
    let json = serde_json::to_string(&request)?;
    write.send(Message::Text(json)).await?;

    tracing::info!("Sent discovery request");

    // Wait for discovery response (with cancellation support)
    let msg_result = tokio::select! {
        _ = stop_rx.recv() => {
            tracing::info!("Discovery cancelled, closing connection");
            let _ = write.close().await;
            return Ok(());
        }
        msg = read.next() => msg,
    };

    if let Some(msg_result) = msg_result {
        match msg_result {
            Ok(Message::Text(text)) => {
                tracing::debug!("Discovery response: {}", text);

                match serde_json::from_str::<crate::protocol::Response>(&text) {
                    Ok(response) => {
                        if response.has_errors() {
                            tracing::error!("Discovery response has errors: {:?}", response.errors);
                            let _ = write.close().await;
                            return Ok(());
                        }

                        if let Some(data) = &response.data {
                            // Parse speaker IPs from data.local
                            if let Some(speaker_ips) = parse_speaker_ips(data) {
                                tracing::info!("Found {} speaker IP(s)", speaker_ips.len());

                                if speaker_ips.is_empty() {
                                    tracing::warn!("No speakers found in discovery response");
                                    let _ = write.close().await;
                                    return Ok(());
                                }

                                // Process each speaker
                                for speaker_ip in speaker_ips {
                                    if let Err(e) = process_speaker(&speaker_ip, speakers, rooms, update_tx).await {
                                        tracing::warn!("Failed to process speaker at {}: {}", speaker_ip, e);
                                    }
                                }
                            } else {
                                tracing::warn!("Could not parse speaker IPs from discovery response");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to parse discovery response: {}", e);
                    }
                }
            }
            Ok(Message::Close(_)) => {
                tracing::info!("Discovery connection closed by server");
            }
            Err(e) => {
                tracing::error!("WebSocket error: {}", e);
                let _ = write.close().await;
                return Err(e.into());
            }
            _ => {}
        }
    }

    // Close the websocket connection cleanly
    let _ = write.close().await;
    Ok(())
}

fn parse_speaker_ips(data: &serde_json::Value) -> Option<Vec<String>> {
    // Get data.local object
    let local = data.get("local")?.as_object()?;

    tracing::debug!("Found {} speaker(s) in local object", local.len());

    let mut ips = Vec::new();

    for (speaker_id, speaker_data) in local {
        // Get localIp4 array
        if let Some(ip_array) = speaker_data.get("localIp4").and_then(|v| v.as_array()) {
            for ip_val in ip_array {
                if let Some(ip) = ip_val.as_str() {
                    tracing::info!("Found speaker {} at {}", speaker_id, ip);
                    ips.push(ip.to_string());
                }
            }
        }
    }

    if ips.is_empty() {
        None
    } else {
        Some(ips)
    }
}

/// Process a single speaker: connect, get network state, subscribe, and add rooms
async fn process_speaker(
    speaker_ip: &str,
    speakers: &Arc<Mutex<BTreeMap<String, Arc<SpeakerConnection>>>>,
    rooms: &Arc<Mutex<BTreeMap<RoomId, Room>>>,
    update_tx: &Arc<broadcast::Sender<RoomId>>,
) -> Result<()> {
    tracing::info!("Processing speaker at {}", speaker_ip);

    // Check if we already have a connection to this speaker
    let speaker = {
        let speakers_lock = speakers.lock().unwrap();
        if let Some(existing) = speakers_lock.get(speaker_ip) {
            tracing::debug!("Reusing existing connection to {}", speaker_ip);
            Some(existing.clone())
        } else {
            None
        }
    };

    let speaker = if let Some(sp) = speaker {
        sp
    } else {
        // Create new connection (outside of lock)
        tracing::info!("Creating new connection to {}", speaker_ip);
        let conn = SpeakerConnection::connect(speaker_ip.to_string(), SPEAKER_PORT).await?;
        let arc_conn = Arc::new(conn);

        // Insert into map
        {
            let mut speakers_lock = speakers.lock().unwrap();
            speakers_lock.insert(speaker_ip.to_string(), arc_conn.clone());
        }

        arc_conn
    };

    // Request network state
    let network_data = match speaker.request_network_state().await {
        Ok(data) => data,
        Err(e) => {
            tracing::warn!("Failed to get network state from {}: {}", speaker_ip, e);
            return Err(e);
        }
    };

    // Parse rooms from network state
    let parsed_rooms = parse_rooms_from_network_data(&network_data, &speaker)?;

    tracing::info!("Found {} room(s) from speaker at {}", parsed_rooms.len(), speaker_ip);

    // Add rooms to the shared map
    {
        let mut rooms_lock = rooms.lock().unwrap();
        for room in parsed_rooms {
            rooms_lock.insert(room.id(), room);
        }
        tracing::info!("Total rooms in discovery: {}", rooms_lock.len());
    }

    // Subscribe to state updates and spawn background task to process them
    match speaker.subscribe_state().await {
        Ok(mut receiver) => {
            let rooms_clone = rooms.clone();
            let update_tx_clone = update_tx.clone();
            let speaker_clone = speaker.clone();

            tokio::spawn(async move {
                while let Ok(update) = receiver.recv().await {
                    process_state_update(update, &speaker_clone, &rooms_clone, &update_tx_clone).await;
                }
                tracing::debug!("State update receiver closed for speaker");
            });
        }
        Err(e) => {
            tracing::warn!("Failed to subscribe to updates from {}: {}", speaker_ip, e);
        }
    }

    Ok(())
}

/// Process a state update from a speaker
async fn process_state_update(
    update: crate::subscription::StateUpdate,
    speaker: &Arc<SpeakerConnection>,
    rooms: &Arc<Mutex<BTreeMap<RoomId, Room>>>,
    update_tx: &Arc<broadcast::Sender<RoomId>>,
) {
    match update {
        crate::subscription::StateUpdate::RoomUpdate(room_json) => {
            // Extract room ID from JSON
            let room_id = match room_json.get("id")
                .and_then(|v| v.as_str())
                .and_then(|s| uuid::Uuid::parse_str(s).ok())
            {
                Some(id) => id,
                None => {
                    tracing::warn!("Received room update without valid ID");
                    return;
                }
            };

            tracing::debug!("Received room update for {}", room_id);

            let mut rooms_lock = rooms.lock().unwrap();
            if let Some(room) = rooms_lock.get(&room_id) {
                // Update existing room
                if let Err(e) = room.update_from_json(*room_json) {
                    tracing::warn!("Failed to update room {}: {}", room_id, e);
                } else {
                    let _ = update_tx.send(room_id);
                }
            } else {
                // New room discovered via update
                tracing::info!("New room discovered via update: {}", room_id);
                match Room::new(speaker.clone(), *room_json) {
                    Ok(new_room) => {
                        rooms_lock.insert(room_id, new_room);
                        let _ = update_tx.send(room_id);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to create room {}: {}", room_id, e);
                    }
                }
            }
        }
        _ => {
            // Other update types - ignore for now
        }
    }
}

/// Parse rooms from network state data
fn parse_rooms_from_network_data(
    data: &serde_json::Value,
    speaker: &Arc<SpeakerConnection>,
) -> Result<Vec<Room>> {
    tracing::debug!("Parsing rooms from network data");

    // The network endpoint returns data.state as a dictionary
    let state = data
        .get("state")
        .ok_or_else(|| AscendError::InvalidResponse("No state in network response".to_string()))?;

    let state_obj = state
        .as_object()
        .ok_or_else(|| AscendError::InvalidResponse("State is not an object".to_string()))?;

    tracing::debug!("Found {} state entries", state_obj.len());

    let mut rooms = Vec::new();

    for (state_id, state_entry) in state_obj {
        // Check if this is a room
        let data_obj = match state_entry.get("data") {
            Some(d) => d,
            None => continue,
        };

        let type_str = match data_obj.get("type").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => continue,
        };

        if type_str != "room" {
            tracing::debug!("State entry {} is type {}, skipping", state_id, type_str);
            continue;
        }

        // This is a room, parse it
        match Room::new(speaker.clone(), data_obj.clone()) {
            Ok(room) => {
                tracing::info!("Discovered room: {} ({})", room.name(), room.id());
                rooms.push(room);
            }
            Err(e) => {
                tracing::warn!("Failed to parse room {}: {}", state_id, e);
            }
        }
    }

    Ok(rooms)
}
