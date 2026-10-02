use std::collections::BTreeMap;

use crate::protocol::openai_chat::Usage;
use crate::protocol::openai_chat::stream::{Chunk, ToolCallDelta};

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Meta { id: String, model: String },
    TextDelta { delta: String },
    ToolCallStart { id: String, name: String },
    ToolCallArgs { id: String, delta: String },
    Usage { usage: Usage },
    Finish { reason: Option<String> },
}

#[derive(Default)]
struct Tool {
    id: Option<String>,
    name: Option<String>,
    announced: bool,
    buffered: String,
}

#[derive(Default)]
pub struct Hub {
    started: bool,
    id: String,
    model: String,
    tools: BTreeMap<u32, Tool>,
    finish_reason: Option<String>,
}

impl Hub {
    pub fn feed(&mut self, chunk: &Chunk) -> Vec<Event> {
        let mut out = Vec::new();
        if !self.started {
            self.started = true;
            self.id = chunk.id.clone();
            self.model = chunk.model.clone();
            out.push(Event::Meta {
                id: self.id.clone(),
                model: self.model.clone(),
            });
        }

        for choice in &chunk.choices {
            if let Some(content) = &choice.delta.content
                && !content.is_empty()
            {
                out.push(Event::TextDelta {
                    delta: content.clone(),
                });
            }
            if let Some(calls) = &choice.delta.tool_calls {
                for call in calls {
                    out.extend(self.on_tool_call(call));
                }
            }
            if let Some(reason) = &choice.finish_reason
                && self.finish_reason.is_none()
            {
                self.finish_reason = Some(reason.clone());
            }
        }

        if let Some(usage) = &chunk.usage {
            out.push(Event::Usage { usage: *usage });
        }

        out
    }

    pub fn finish(&self) -> Vec<Event> {
        vec![Event::Finish {
            reason: self.finish_reason.clone(),
        }]
    }

    fn on_tool_call(&mut self, call: &ToolCallDelta) -> Vec<Event> {
        let arguments = call.function.as_ref().and_then(|f| f.arguments.clone());
        let tool = self.tools.entry(call.index).or_default();
        if tool.id.is_none() {
            tool.id = call.id.clone().filter(|id| !id.is_empty());
        }
        if tool.name.is_none() {
            tool.name = call.function.as_ref().and_then(|f| f.name.clone());
        }

        if tool.announced {
            let arguments = arguments.unwrap_or_default();
            if arguments.is_empty() {
                return Vec::new();
            }
            let Some(id) = tool.id.clone() else {
                return Vec::new();
            };
            return vec![Event::ToolCallArgs {
                id,
                delta: arguments,
            }];
        }

        tool.buffered
            .push_str(arguments.as_deref().unwrap_or_default());
        let (Some(id), Some(name)) = (tool.id.clone(), tool.name.clone()) else {
            return Vec::new();
        };
        tool.announced = true;
        let buffered = std::mem::take(&mut tool.buffered);
        let mut out = vec![Event::ToolCallStart {
            id: id.clone(),
            name,
        }];
        if !buffered.is_empty() {
            out.push(Event::ToolCallArgs {
                id,
                delta: buffered,
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::openai_chat::stream::{
        Chunk, ChunkChoice, Delta, FunctionDelta, ToolCallDelta,
    };

    fn chunk(content: Option<&str>, calls: &[ToolCallDelta], finish: Option<&str>) -> Chunk {
        Chunk {
            id: "c".into(),
            model: "m".into(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: Delta {
                    content: content.map(String::from),
                    tool_calls: (!calls.is_empty()).then(|| calls.to_vec()),
                },
                finish_reason: finish.map(String::from),
            }],
            usage: None,
        }
    }

    fn call(index: u32, id: Option<&str>, name: Option<&str>, arguments: &str) -> ToolCallDelta {
        ToolCallDelta {
            index,
            id: id.map(String::from),
            function: Some(FunctionDelta {
                name: name.map(String::from),
                arguments: Some(arguments.to_string()),
            }),
        }
    }

    #[test]
    fn the_first_chunk_emits_meta_then_text() {
        let mut hub = Hub::default();
        assert_eq!(
            hub.feed(&chunk(Some("hi"), &[], None)),
            vec![
                Event::Meta {
                    id: "c".into(),
                    model: "m".into(),
                },
                Event::TextDelta { delta: "hi".into() },
            ]
        );
    }

    #[test]
    fn a_tool_call_named_late_announces_once_and_flushes_the_buffered_arguments() {
        let mut hub = Hub::default();
        hub.feed(&chunk(None, &[call(0, None, None, "{\"ci")], None));
        assert_eq!(
            hub.feed(&chunk(
                None,
                &[call(0, Some("call_1"), Some("f"), "ty\": \"")],
                None
            )),
            vec![
                Event::ToolCallStart {
                    id: "call_1".into(),
                    name: "f".into(),
                },
                Event::ToolCallArgs {
                    id: "call_1".into(),
                    delta: "{\"city\": \"".into(),
                },
            ]
        );
        assert_eq!(
            hub.feed(&chunk(None, &[call(0, None, None, "X\"}")], None)),
            vec![Event::ToolCallArgs {
                id: "call_1".into(),
                delta: "X\"}".into(),
            }]
        );
    }

    #[test]
    fn a_tool_call_the_upstream_never_names_is_not_announced() {
        let mut hub = Hub::default();
        assert_eq!(
            hub.feed(&chunk(None, &[call(0, None, None, "{}")], None)),
            vec![Event::Meta {
                id: "c".into(),
                model: "m".into(),
            }]
        );
        assert_eq!(hub.finish(), vec![Event::Finish { reason: None }]);
    }

    #[test]
    fn a_tool_call_announces_exactly_once_when_later_deltas_return_to_it() {
        let mut hub = Hub::default();
        hub.feed(&chunk(
            None,
            &[call(0, Some("call_0"), Some("f0"), "")],
            None,
        ));
        hub.feed(&chunk(
            None,
            &[call(1, Some("call_1"), Some("f1"), "")],
            None,
        ));
        assert_eq!(
            hub.feed(&chunk(None, &[call(0, None, None, "{}")], None)),
            vec![Event::ToolCallArgs {
                id: "call_0".into(),
                delta: "{}".into(),
            }]
        );
    }

    #[test]
    fn usage_and_finish_reason_arrive_from_the_terminal_chunk() {
        use crate::protocol::openai_chat::{PromptTokensDetails, Usage};

        let mut hub = Hub::default();
        let mut terminal = chunk(None, &[], Some("tool_calls"));
        terminal.usage = Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            prompt_tokens_details: Some(PromptTokensDetails {
                cached_tokens: 2,
                cache_write_tokens: 1,
            }),
        });

        assert_eq!(
            hub.feed(&terminal),
            vec![
                Event::Meta {
                    id: "c".into(),
                    model: "m".into(),
                },
                Event::Usage {
                    usage: Usage {
                        prompt_tokens: 10,
                        completion_tokens: 5,
                        total_tokens: 15,
                        prompt_tokens_details: Some(PromptTokensDetails {
                            cached_tokens: 2,
                            cache_write_tokens: 1,
                        }),
                    },
                },
            ]
        );
        assert_eq!(
            hub.finish(),
            vec![Event::Finish {
                reason: Some("tool_calls".into()),
            }]
        );
    }

    #[test]
    fn the_vendored_chat_streams_feed_the_hub_without_violating_its_contract() {
        use std::collections::BTreeSet;

        use crate::protocol::corpus::CHAT_STREAMS;
        use crate::protocol::test_support::frames;

        for (name, text) in CHAT_STREAMS {
            let mut hub = Hub::default();
            let mut announced = BTreeSet::new();
            let mut open = BTreeSet::new();
            let mut first = true;
            let mut meta_seen = false;
            let mut usage_seen = false;

            for frame in frames(text) {
                let chunk: Chunk = serde_json::from_value(frame).expect("a fixture decodes");
                for event in hub.feed(&chunk) {
                    if first {
                        first = false;
                        assert!(
                            matches!(&event, Event::Meta { .. }),
                            "{name}: meta is not first"
                        );
                    }
                    match event {
                        Event::Meta { .. } => {
                            assert!(!meta_seen, "{name}: meta emitted twice");
                            meta_seen = true;
                        }
                        Event::TextDelta { delta } => {
                            assert!(!delta.is_empty(), "{name}: empty text delta");
                        }
                        Event::ToolCallStart { id, name } => {
                            assert!(
                                !id.is_empty() && !name.is_empty(),
                                "{name}: an unnamed call was announced"
                            );
                            assert!(
                                announced.insert(id.clone()),
                                "{name}: call {id} announced twice"
                            );
                            open.insert(id);
                        }
                        Event::ToolCallArgs { id, .. } => {
                            assert!(
                                open.contains(&id),
                                "{name}: arguments for an unannounced call {id}"
                            );
                        }
                        Event::Usage { .. } => {
                            assert!(!usage_seen, "{name}: usage emitted twice");
                            usage_seen = true;
                        }
                        Event::Finish { .. } => {
                            panic!("{name}: finish arrived before the end");
                        }
                    }
                }
            }

            assert_eq!(hub.finish().len(), 1, "{name}: finish is one event");
        }
    }
}
