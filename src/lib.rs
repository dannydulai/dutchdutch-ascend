//! Rust library for controlling Dutch and Dutch Ascend networked speakers
//!
//! This library provides an async API for discovering and controlling Dutch and Dutch
//! Ascend speaker systems. It supports:
//!
//! - Discovery via Ascend Cloud API
//! - Room control via local WebSocket connection
//! - Volume and mute control (global and per-position)
//! - Voicing profile selection and tone adjustment
//! - Preset management
//! - Channel mapping configuration
//! - Real-time state update subscriptions
//!
//! # Quick Start
//!
//! ```no_run
//! use dutchdutch_ascend::Discovery;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Start discovery
//!     let mut discovery = Discovery::new();
//!     discovery.start().await?;
//!
//!     // Wait for rooms to be discovered
//!     tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
//!
//!     // Get discovered rooms
//!     let rooms = discovery.rooms();
//!     if let Some(discovered_room) = rooms.first() {
//!         println!("Found room: {}", discovered_room.name);
//!
//!         // Connect to the discovered room
//!         let client = discovered_room.connect().await?;
//!         let room_objs = client.rooms().await?;
//!
//!         if let Some(room) = room_objs.first() {
//!             // Control the room
//!             room.set_gain(-20.0).await?;
//!             room.set_mute(false).await?;
//!
//!             // Subscribe to state updates
//!             let mut updates = client.subscribe_state().await?;
//!             while let Ok(update) = updates.recv().await {
//!                 println!("State update: {:?}", update);
//!                 break; // Just show one update
//!             }
//!         }
//!     }
//!
//!     discovery.stop().await;
//!     Ok(())
//! }
//! ```
//!
//! # Direct Connection
//!
//! If you know the IP address of a speaker, you can connect directly:
//!
//! ```no_run
//! use dutchdutch_ascend::AscendClient;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = AscendClient::connect("192.168.1.100", 8768).await?;
//!     let rooms = client.rooms().await?;
//!     if let Some(room) = rooms.first() {
//!         room.set_gain(-15.0).await?;
//!     }
//!     Ok(())
//! }
//! ```
//!
//! # Architecture
//!
//! The library is organized into several layers:
//!
//! - **Discovery**: Cloud-based room discovery via `wss://api.ascend.audio/`
//! - **Client**: Connection management and room access
//! - **Room**: High-level control API for speaker systems
//! - **Connection**: Low-level WebSocket protocol handling
//! - **Protocol**: JSON message structures
//! - **Types**: Domain types and data structures

mod client;
mod connection;
mod discovery;
mod error;
mod protocol;
mod room;
mod speaker_connection;
mod subscription;
mod types;

// Public exports
pub use client::AscendClient;
pub use discovery::Discovery;
pub use error::{AscendError, Result};
pub use room::{Room, RoomState};
pub use subscription::{StateReceiver, StateUpdate};
pub use types::{
    ChannelGains, ChannelMapping, Device, DeviceId, DiscoveredRoom, GainData, GainLimits,
    GainValue, MuteData, MuteState, PositionId, Preset, RoomId, ToneSettings,
    VoicingProfile,
};
