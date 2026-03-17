//! Helpers for truncating rollouts based on "user turn" boundaries.
//!
//! In core, "user turns" are detected by scanning `ResponseItem::Message` items and
//! interpreting them via `event_mapping::parse_turn_item(...)`.

use crate::event_mapping;
use codex_protocol::items::TurnItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::RolloutItem;

fn turn_ids_are_compatible(active_turn_id: Option<&str>, item_turn_id: Option<&str>) -> bool {
    active_turn_id
        .is_none_or(|turn_id| item_turn_id.is_none_or(|item_turn_id| item_turn_id == turn_id))
}

/// Return the indices of user message boundaries in a rollout.
///
/// A user message boundary is a `RolloutItem::ResponseItem(ResponseItem::Message { .. })`
/// whose parsed turn item is `TurnItem::UserMessage`.
///
/// Rollouts can contain `ThreadRolledBack` markers. Those markers indicate that the
/// last N user turns were removed from the effective thread history; we apply them here so
/// indexing uses the post-rollback history rather than the raw stream.
pub(crate) fn user_message_positions_in_rollout(items: &[RolloutItem]) -> Vec<usize> {
    let mut user_positions = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        match item {
            RolloutItem::ResponseItem(item @ ResponseItem::Message { .. })
                if matches!(
                    event_mapping::parse_turn_item(item),
                    Some(TurnItem::UserMessage(_))
                ) =>
            {
                user_positions.push(idx);
            }
            RolloutItem::EventMsg(EventMsg::ThreadRolledBack(rollback)) => {
                let num_turns = usize::try_from(rollback.num_turns).unwrap_or(usize::MAX);
                let new_len = user_positions.len().saturating_sub(num_turns);
                user_positions.truncate(new_len);
            }
            _ => {}
        }
    }
    user_positions
}

/// Return a prefix of `items` obtained by cutting strictly before the nth user message.
///
/// The boundary index is 0-based from the start of `items` (so `n_from_start = 0` returns
/// a prefix that excludes the first user message and everything after it).
///
/// If `n_from_start` is `usize::MAX`, this returns the full rollout (no truncation).
/// If fewer than or equal to `n_from_start` user messages exist, this returns an empty
/// vector (out of range).
pub(crate) fn truncate_rollout_before_nth_user_message_from_start(
    items: &[RolloutItem],
    n_from_start: usize,
) -> Vec<RolloutItem> {
    if n_from_start == usize::MAX {
        return items.to_vec();
    }

    let user_positions = user_message_positions_in_rollout(items);

    // If fewer than or equal to n user messages exist, treat as empty (out of range).
    if user_positions.len() <= n_from_start {
        return Vec::new();
    }

    // Cut strictly before the nth user message (do not keep the nth itself).
    let cut_idx = user_positions[n_from_start];
    items[..cut_idx].to_vec()
}

/// Return a prefix of `items` obtained by cutting strictly before the last user message.
///
/// This is useful when a child thread should inherit completed background context from a parent
/// thread without also inheriting the parent's current live directive as model-visible input.
///
/// If no user messages exist, this returns the full rollout (no truncation).
pub(crate) fn truncate_rollout_before_last_user_message(items: &[RolloutItem]) -> Vec<RolloutItem> {
    let user_positions = user_message_positions_in_rollout(items);
    let Some(cut_idx) = user_positions.last().copied() else {
        return items.to_vec();
    };

    items[..cut_idx].to_vec()
}

/// Return a prefix of `items` obtained by cutting strictly before the newest surviving
/// user-turn segment.
///
/// When turn events are present, this trims at the segment start rather than at the raw user
/// message item, so incomplete trailing turn scaffolding such as `TurnStarted` does not leak
/// into a forked child rollout. Legacy rollouts without turn events fall back to trimming before
/// the last user message.
pub(crate) fn truncate_rollout_before_last_user_turn_segment(
    items: &[RolloutItem],
) -> Vec<RolloutItem> {
    #[derive(Default)]
    struct ActiveUserTurnSegment {
        start_idx: usize,
        turn_id: Option<String>,
        counts_as_user_turn: bool,
    }

    let mut saw_turn_events = false;
    let mut user_turn_segment_starts = Vec::new();
    let mut active_segment: Option<ActiveUserTurnSegment> = None;

    let finalize_active_segment =
        |active_segment: &mut Option<ActiveUserTurnSegment>,
         user_turn_segment_starts: &mut Vec<usize>| {
            if let Some(active_segment) = active_segment.take()
                && active_segment.counts_as_user_turn
            {
                user_turn_segment_starts.push(active_segment.start_idx);
            }
        };

    for (idx, item) in items.iter().enumerate() {
        match item {
            RolloutItem::EventMsg(EventMsg::TurnStarted(event)) => {
                saw_turn_events = true;
                finalize_active_segment(&mut active_segment, &mut user_turn_segment_starts);
                active_segment = Some(ActiveUserTurnSegment {
                    start_idx: idx,
                    turn_id: Some(event.turn_id.clone()),
                    counts_as_user_turn: false,
                });
            }
            RolloutItem::EventMsg(EventMsg::UserMessage(_)) => {
                saw_turn_events = true;
                let active_segment = active_segment.get_or_insert_with(|| ActiveUserTurnSegment {
                    start_idx: idx,
                    ..Default::default()
                });
                active_segment.counts_as_user_turn = true;
            }
            RolloutItem::TurnContext(ctx) => {
                let active_segment = active_segment.get_or_insert_with(|| ActiveUserTurnSegment {
                    start_idx: idx,
                    ..Default::default()
                });
                if active_segment.turn_id.is_none() {
                    active_segment.turn_id = ctx.turn_id.clone();
                }
            }
            RolloutItem::ResponseItem(item @ ResponseItem::Message { .. })
                if matches!(
                    event_mapping::parse_turn_item(item),
                    Some(TurnItem::UserMessage(_))
                ) =>
            {
                if saw_turn_events {
                    let active_segment =
                        active_segment.get_or_insert_with(|| ActiveUserTurnSegment {
                            start_idx: idx,
                            ..Default::default()
                        });
                    active_segment.counts_as_user_turn = true;
                }
            }
            RolloutItem::EventMsg(EventMsg::TurnComplete(event)) => {
                saw_turn_events = true;
                if active_segment.as_ref().is_some_and(|active_segment| {
                    turn_ids_are_compatible(
                        active_segment.turn_id.as_deref(),
                        Some(event.turn_id.as_str()),
                    )
                }) {
                    finalize_active_segment(&mut active_segment, &mut user_turn_segment_starts);
                }
            }
            RolloutItem::EventMsg(EventMsg::TurnAborted(event)) => {
                saw_turn_events = true;
                if active_segment.as_ref().is_some_and(|active_segment| {
                    turn_ids_are_compatible(
                        active_segment.turn_id.as_deref(),
                        event.turn_id.as_deref(),
                    )
                }) {
                    finalize_active_segment(&mut active_segment, &mut user_turn_segment_starts);
                }
            }
            RolloutItem::EventMsg(EventMsg::ThreadRolledBack(rollback)) => {
                let num_turns = usize::try_from(rollback.num_turns).unwrap_or(usize::MAX);
                let new_len = user_turn_segment_starts.len().saturating_sub(num_turns);
                user_turn_segment_starts.truncate(new_len);
            }
            _ => {}
        }
    }

    if saw_turn_events {
        finalize_active_segment(&mut active_segment, &mut user_turn_segment_starts);
        let Some(cut_idx) = user_turn_segment_starts.last().copied() else {
            return items.to_vec();
        };
        return items[..cut_idx].to_vec();
    }

    truncate_rollout_before_last_user_message(items)
}

#[cfg(test)]
#[path = "truncation_tests.rs"]
mod tests;
