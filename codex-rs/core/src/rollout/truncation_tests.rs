use super::*;
use crate::codex::make_session_and_context;
use assert_matches::assert_matches;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::protocol::ThreadRolledBackEvent;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::protocol::UserMessageEvent;
use pretty_assertions::assert_eq;

fn user_msg(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::OutputText {
            text: text.to_string(),
        }],
        end_turn: None,
        phase: None,
    }
}

fn assistant_msg(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText {
            text: text.to_string(),
        }],
        end_turn: None,
        phase: None,
    }
}

#[test]
fn truncates_rollout_from_start_before_nth_user_only() {
    let items = [
        user_msg("u1"),
        assistant_msg("a1"),
        assistant_msg("a2"),
        user_msg("u2"),
        assistant_msg("a3"),
        ResponseItem::Reasoning {
            id: "r1".to_string(),
            summary: vec![ReasoningItemReasoningSummary::SummaryText {
                text: "s".to_string(),
            }],
            content: None,
            encrypted_content: None,
        },
        ResponseItem::FunctionCall {
            id: None,
            call_id: "c1".to_string(),
            name: "tool".to_string(),
            namespace: None,
            arguments: "{}".to_string(),
        },
        assistant_msg("a4"),
    ];

    let rollout: Vec<RolloutItem> = items
        .iter()
        .cloned()
        .map(RolloutItem::ResponseItem)
        .collect();

    let truncated = truncate_rollout_before_nth_user_message_from_start(&rollout, 1);
    let expected = vec![
        RolloutItem::ResponseItem(items[0].clone()),
        RolloutItem::ResponseItem(items[1].clone()),
        RolloutItem::ResponseItem(items[2].clone()),
    ];
    assert_eq!(
        serde_json::to_value(&truncated).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );

    let truncated2 = truncate_rollout_before_nth_user_message_from_start(&rollout, 2);
    assert_matches!(truncated2.as_slice(), []);
}

#[test]
fn truncation_max_keeps_full_rollout() {
    let rollout = vec![
        RolloutItem::ResponseItem(user_msg("u1")),
        RolloutItem::ResponseItem(assistant_msg("a1")),
        RolloutItem::ResponseItem(user_msg("u2")),
    ];

    let truncated = truncate_rollout_before_nth_user_message_from_start(&rollout, usize::MAX);

    assert_eq!(
        serde_json::to_value(&truncated).unwrap(),
        serde_json::to_value(&rollout).unwrap()
    );
}

#[test]
fn truncates_rollout_before_last_user_message_only() {
    let rollout = vec![
        RolloutItem::ResponseItem(user_msg("u1")),
        RolloutItem::ResponseItem(assistant_msg("a1")),
        RolloutItem::ResponseItem(user_msg("u2")),
        RolloutItem::ResponseItem(assistant_msg("a2")),
        RolloutItem::ResponseItem(ResponseItem::FunctionCall {
            id: None,
            call_id: "c1".to_string(),
            name: "spawn_agent".to_string(),
            namespace: None,
            arguments: "{}".to_string(),
        }),
    ];

    let truncated = truncate_rollout_before_last_user_message(&rollout);
    let expected = vec![
        RolloutItem::ResponseItem(user_msg("u1")),
        RolloutItem::ResponseItem(assistant_msg("a1")),
    ];

    assert_eq!(
        serde_json::to_value(&truncated).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
}

#[test]
fn truncate_before_last_user_keeps_full_rollout_without_user_messages() {
    let rollout = vec![
        RolloutItem::ResponseItem(assistant_msg("a1")),
        RolloutItem::ResponseItem(ResponseItem::FunctionCall {
            id: None,
            call_id: "c1".to_string(),
            name: "spawn_agent".to_string(),
            namespace: None,
            arguments: "{}".to_string(),
        }),
    ];

    let truncated = truncate_rollout_before_last_user_message(&rollout);

    assert_eq!(
        serde_json::to_value(&truncated).unwrap(),
        serde_json::to_value(&rollout).unwrap()
    );
}

#[test]
fn truncates_rollout_before_last_user_turn_segment_start_when_turn_events_exist() {
    let rollout = vec![
        RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-1".to_string(),
            model_context_window: None,
            collaboration_mode_kind: codex_protocol::config_types::ModeKind::Default,
        })),
        RolloutItem::EventMsg(EventMsg::UserMessage(UserMessageEvent {
            message: "u1".to_string(),
            images: None,
            local_images: Vec::new(),
            text_elements: Vec::new(),
        })),
        RolloutItem::ResponseItem(assistant_msg("a1")),
        RolloutItem::EventMsg(EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-1".to_string(),
            last_agent_message: None,
        })),
        RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-2".to_string(),
            model_context_window: None,
            collaboration_mode_kind: codex_protocol::config_types::ModeKind::Default,
        })),
        RolloutItem::EventMsg(EventMsg::UserMessage(UserMessageEvent {
            message: "u2".to_string(),
            images: None,
            local_images: Vec::new(),
            text_elements: Vec::new(),
        })),
        RolloutItem::ResponseItem(assistant_msg("a2")),
        RolloutItem::ResponseItem(ResponseItem::FunctionCall {
            id: None,
            call_id: "spawn-1".to_string(),
            name: "spawn_agent".to_string(),
            namespace: None,
            arguments: "{}".to_string(),
        }),
    ];

    let truncated = truncate_rollout_before_last_user_turn_segment(&rollout);
    let expected = rollout[..4].to_vec();

    assert_eq!(
        serde_json::to_value(&truncated).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
}

#[test]
fn truncates_rollout_before_last_user_turn_segment_applies_thread_rollback_markers() {
    let rollout = vec![
        RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-1".to_string(),
            model_context_window: None,
            collaboration_mode_kind: codex_protocol::config_types::ModeKind::Default,
        })),
        RolloutItem::EventMsg(EventMsg::UserMessage(UserMessageEvent {
            message: "u1".to_string(),
            images: None,
            local_images: Vec::new(),
            text_elements: Vec::new(),
        })),
        RolloutItem::EventMsg(EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-1".to_string(),
            last_agent_message: None,
        })),
        RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-2".to_string(),
            model_context_window: None,
            collaboration_mode_kind: codex_protocol::config_types::ModeKind::Default,
        })),
        RolloutItem::EventMsg(EventMsg::UserMessage(UserMessageEvent {
            message: "u2".to_string(),
            images: None,
            local_images: Vec::new(),
            text_elements: Vec::new(),
        })),
        RolloutItem::EventMsg(EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-2".to_string(),
            last_agent_message: None,
        })),
        RolloutItem::EventMsg(EventMsg::ThreadRolledBack(ThreadRolledBackEvent {
            num_turns: 1,
        })),
        RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-3".to_string(),
            model_context_window: None,
            collaboration_mode_kind: codex_protocol::config_types::ModeKind::Default,
        })),
        RolloutItem::EventMsg(EventMsg::UserMessage(UserMessageEvent {
            message: "u3".to_string(),
            images: None,
            local_images: Vec::new(),
            text_elements: Vec::new(),
        })),
        RolloutItem::ResponseItem(ResponseItem::FunctionCall {
            id: None,
            call_id: "spawn-3".to_string(),
            name: "spawn_agent".to_string(),
            namespace: None,
            arguments: "{}".to_string(),
        }),
    ];

    let truncated = truncate_rollout_before_last_user_turn_segment(&rollout);
    let expected = rollout[..7].to_vec();

    assert_eq!(
        serde_json::to_value(&truncated).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
}

#[test]
fn truncates_rollout_from_start_applies_thread_rollback_markers() {
    let rollout_items = vec![
        RolloutItem::ResponseItem(user_msg("u1")),
        RolloutItem::ResponseItem(assistant_msg("a1")),
        RolloutItem::ResponseItem(user_msg("u2")),
        RolloutItem::ResponseItem(assistant_msg("a2")),
        RolloutItem::EventMsg(EventMsg::ThreadRolledBack(ThreadRolledBackEvent {
            num_turns: 1,
        })),
        RolloutItem::ResponseItem(user_msg("u3")),
        RolloutItem::ResponseItem(assistant_msg("a3")),
        RolloutItem::ResponseItem(user_msg("u4")),
        RolloutItem::ResponseItem(assistant_msg("a4")),
    ];

    // Effective user history after applying rollback(1) is: u1, u3, u4.
    // So n_from_start=2 should cut before u4 (not u3).
    let truncated = truncate_rollout_before_nth_user_message_from_start(&rollout_items, 2);
    let expected = rollout_items[..7].to_vec();
    assert_eq!(
        serde_json::to_value(&truncated).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
}

#[tokio::test]
async fn ignores_session_prefix_messages_when_truncating_rollout_from_start() {
    let (session, turn_context) = make_session_and_context().await;
    let mut items = session.build_initial_context(&turn_context).await;
    items.push(user_msg("feature request"));
    items.push(assistant_msg("ack"));
    items.push(user_msg("second question"));
    items.push(assistant_msg("answer"));

    let rollout_items: Vec<RolloutItem> = items
        .iter()
        .cloned()
        .map(RolloutItem::ResponseItem)
        .collect();

    let truncated = truncate_rollout_before_nth_user_message_from_start(&rollout_items, 1);
    let expected: Vec<RolloutItem> = vec![
        RolloutItem::ResponseItem(items[0].clone()),
        RolloutItem::ResponseItem(items[1].clone()),
        RolloutItem::ResponseItem(items[2].clone()),
        RolloutItem::ResponseItem(items[3].clone()),
    ];

    assert_eq!(
        serde_json::to_value(&truncated).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
}
