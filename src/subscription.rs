use crate::error::{AscendError, Result};
use crate::types::{Device, DeviceId};
use tokio::sync::broadcast;

/// State update from a subscription
#[derive(Debug, Clone)]
pub enum StateUpdate {
    /// Room state was updated (raw JSON)
    RoomUpdate(Box<serde_json::Value>),

    /// Device state was updated
    DeviceUpdate(DeviceId, Device),
}

/// Receiver for state updates
pub struct StateReceiver {
    rx: broadcast::Receiver<StateUpdate>,
}

impl StateReceiver {
    /// Create a new state receiver
    pub(crate) fn new(rx: broadcast::Receiver<StateUpdate>) -> Self {
        Self { rx }
    }

    /// Receive the next state update
    ///
    /// Returns `None` if all senders have been dropped (connection closed).
    pub async fn recv(&mut self) -> Result<StateUpdate> {
        self.rx
            .recv()
            .await
            .map_err(|e| match e {
                broadcast::error::RecvError::Closed => AscendError::ConnectionClosed,
                broadcast::error::RecvError::Lagged(n) => {
                    AscendError::ChannelError(format!("Lagged by {} messages", n))
                }
            })
    }

    /// Try to receive a state update without blocking
    ///
    /// Returns `None` if no message is available.
    pub fn try_recv(&mut self) -> Result<Option<StateUpdate>> {
        match self.rx.try_recv() {
            Ok(update) => Ok(Some(update)),
            Err(broadcast::error::TryRecvError::Empty) => Ok(None),
            Err(broadcast::error::TryRecvError::Closed) => Err(AscendError::ConnectionClosed),
            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                Err(AscendError::ChannelError(format!("Lagged by {} messages", n)))
            }
        }
    }
}
