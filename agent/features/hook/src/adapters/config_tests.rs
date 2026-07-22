#![cfg(test)]

use std::collections::HashMap;
use std::time::Duration;

use share::config::hooks::{HookEntry, HookEvent, HooksConfig};

use crate::adapters::config::subscriptions_from_config;
use crate::{HookMatcher, HookPoint};

#[test]
fn subscriptions_from_config_preserves_event_entry_order_and_wire_fields() {
    let config = HooksConfig {
        events: HashMap::from([
            (
                HookEvent::PreToolUse,
                vec![
                    HookEntry {
                        matcher: String::new(),
                        command: "first".to_string(),
                        timeout: 7,
                    },
                    HookEntry {
                        matcher: "Bash".to_string(),
                        command: "second".to_string(),
                        timeout: 9,
                    },
                ],
            ),
            (
                HookEvent::Stop,
                vec![HookEntry {
                    matcher: String::new(),
                    command: "stop".to_string(),
                    timeout: 11,
                }],
            ),
        ]),
    };

    let subscriptions = subscriptions_from_config(&config);
    let pre_tool = subscriptions
        .iter()
        .filter(|subscription| subscription.point == HookPoint::PreToolUse)
        .collect::<Vec<_>>();

    assert_eq!(pre_tool.len(), 2);
    assert_eq!(pre_tool[0].matcher, HookMatcher::All);
    assert_eq!(pre_tool[0].command.command, "first");
    assert_eq!(pre_tool[0].timeout, Duration::from_secs(7));
    assert_eq!(pre_tool[0].order, 0);
    assert!(pre_tool[0].enabled);
    assert_eq!(
        pre_tool[1].matcher,
        HookMatcher::ToolName("Bash".to_string())
    );
    assert_eq!(pre_tool[1].command.command, "second");
    assert_eq!(pre_tool[1].timeout, Duration::from_secs(9));
    assert_eq!(pre_tool[1].order, 1);
    assert_eq!(
        subscriptions
            .iter()
            .find(|subscription| subscription.point == HookPoint::Stop)
            .expect("Stop subscription")
            .command
            .command,
        "stop"
    );
}

#[test]
fn subscriptions_from_config_maps_subagent_compatibility_points() {
    let config = HooksConfig {
        events: HashMap::from([(
            HookEvent::SubagentStart,
            vec![HookEntry {
                matcher: String::new(),
                command: "start".to_string(),
                timeout: 60,
            }],
        )]),
    };

    let subscriptions = subscriptions_from_config(&config);

    assert_eq!(subscriptions.len(), 1);
    assert_eq!(subscriptions[0].point, HookPoint::SubRunStart);
}
