use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// Events emitted by process components for observability and coordination.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProcessEvent {
    WorkerStarted {
        entity_id: String,
        turn: u32,
    },
    ToolExecuted {
        entity_id: String,
        tool_name: String,
        success: bool,
    },
    WorkerCompleted {
        entity_id: String,
        turn: u32,
        tokens_used: Option<u64>,
    },
    ContextCompacted {
        entity_id: String,
        level: String,
    },
    CortexBulletinUpdated,
    BranchCreated {
        entity_id: String,
    },
    BranchClosed {
        entity_id: String,
    },
    Error {
        entity_id: Option<String>,
        message: String,
    },
}

pub type EventSender = broadcast::Sender<ProcessEvent>;
pub type EventReceiver = broadcast::Receiver<ProcessEvent>;

/// Create a broadcast event bus with the given capacity.
pub fn event_bus(capacity: usize) -> (EventSender, EventReceiver) {
    broadcast::channel(capacity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_bus_creation() {
        let (tx, _rx) = event_bus(16);
        // Sender should report zero receivers after the initial one is dropped.
        // But let's verify creation doesn't panic.
        assert_eq!(tx.receiver_count(), 1);
    }

    #[tokio::test]
    async fn event_bus_send_receive() {
        let (tx, mut rx) = event_bus(16);

        tx.send(ProcessEvent::WorkerStarted {
            entity_id: "user:1".to_string(),
            turn: 1,
        })
        .unwrap();

        let event = rx.recv().await.unwrap();
        match event {
            ProcessEvent::WorkerStarted { entity_id, turn } => {
                assert_eq!(entity_id, "user:1");
                assert_eq!(turn, 1);
            }
            _ => panic!("expected WorkerStarted event"),
        }
    }

    #[tokio::test]
    async fn event_bus_multiple_events() {
        let (tx, mut rx) = event_bus(16);

        tx.send(ProcessEvent::BranchCreated {
            entity_id: "user:a".to_string(),
        })
        .unwrap();

        tx.send(ProcessEvent::ToolExecuted {
            entity_id: "user:a".to_string(),
            tool_name: "shell".to_string(),
            success: true,
        })
        .unwrap();

        tx.send(ProcessEvent::BranchClosed {
            entity_id: "user:a".to_string(),
        })
        .unwrap();

        let e1 = rx.recv().await.unwrap();
        assert!(matches!(e1, ProcessEvent::BranchCreated { .. }));

        let e2 = rx.recv().await.unwrap();
        assert!(matches!(
            e2,
            ProcessEvent::ToolExecuted { success: true, .. }
        ));

        let e3 = rx.recv().await.unwrap();
        assert!(matches!(e3, ProcessEvent::BranchClosed { .. }));
    }

    #[tokio::test]
    async fn event_bus_multiple_receivers() {
        let (tx, mut rx1) = event_bus(16);
        let mut rx2 = tx.subscribe();

        tx.send(ProcessEvent::CortexBulletinUpdated).unwrap();

        let e1 = rx1.recv().await.unwrap();
        let e2 = rx2.recv().await.unwrap();

        assert!(matches!(e1, ProcessEvent::CortexBulletinUpdated));
        assert!(matches!(e2, ProcessEvent::CortexBulletinUpdated));
    }

    #[test]
    fn process_event_serde_round_trip() {
        let event = ProcessEvent::WorkerCompleted {
            entity_id: "test".to_string(),
            turn: 5,
            tokens_used: Some(1234),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: ProcessEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ProcessEvent::WorkerCompleted {
                entity_id,
                turn,
                tokens_used,
            } => {
                assert_eq!(entity_id, "test");
                assert_eq!(turn, 5);
                assert_eq!(tokens_used, Some(1234));
            }
            _ => panic!("expected WorkerCompleted"),
        }
    }

    #[test]
    fn error_event_serde() {
        let event = ProcessEvent::Error {
            entity_id: None,
            message: "something broke".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("something broke"));
        let parsed: ProcessEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            ProcessEvent::Error {
                entity_id: None,
                ..
            }
        ));
    }
}
