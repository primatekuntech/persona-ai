/// SSE broadcast channel for ingest progress events.
use serde::Serialize;
use tokio::sync::broadcast;
use uuid::Uuid;

/// Emitted by workers whenever a document's ingest status changes.
#[derive(Debug, Clone, Serialize)]
pub struct IngestEvent {
    pub user_id: Uuid,
    pub persona_id: Uuid,
    pub document_id: Uuid,
    pub status: String,
    pub progress_pct: Option<i16>,
    pub error: Option<String>,
}

pub type Sender = broadcast::Sender<IngestEvent>;
#[allow(dead_code)]
pub type Receiver = broadcast::Receiver<IngestEvent>;

/// Create a new broadcast channel. The Sender is `Clone`-able and cheap to share.
pub fn new_channel() -> Sender {
    broadcast::channel(256).0
}
