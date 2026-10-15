//! The conversation model every provider codec and the tool loop speak: provider-neutral
//! messages made of text, images, tool calls and tool results.

use serde_json::Value;

/// Longest text or tool-argument JSON kept from one model reply block.
pub const MAX_BLOCK: usize = 1 << 20;

/// Most blocks kept from one model reply.
pub const MAX_BLOCKS: usize = 256;

/// A part of a tool result.
#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Text(String),
    /// Base64 image data.
    Image {
        mime: String,
        data: String,
    },
}

/// One block of a message.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Text(String),
    Image {
        mime: String,
        data: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
        /// Why the arguments could not be used (not JSON, not an object); the call is answered
        /// with this error and never run.
        error: Option<String>,
    },
    ToolResult {
        id: String,
        parts: Vec<Part>,
        is_error: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub role: Role,
    pub blocks: Vec<Block>,
}

impl Message {
    pub fn user_text(text: impl Into<String>) -> Self {
        Self { role: Role::User, blocks: vec![Block::Text(text.into())] }
    }

    pub fn assistant(blocks: Vec<Block>) -> Self {
        Self { role: Role::Assistant, blocks }
    }

    /// The text blocks joined, for display and export.
    pub fn text(&self) -> String {
        let parts: Vec<&str> = self.blocks.iter().filter_map(|b| if let Block::Text(t) = b { Some(t.as_str()) } else { None }).collect();
        parts.join("\n\n")
    }
}

/// Token counts of one or more requests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Usage {
    pub fn add(&mut self, o: &Usage) {
        self.input = self.input.saturating_add(o.input);
        self.output = self.output.saturating_add(o.output);
        self.cache_read = self.cache_read.saturating_add(o.cache_read);
        self.cache_write = self.cache_write.saturating_add(o.cache_write);
    }
}

/// Why a model reply ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    EndTurn,
    ToolUse,
    MaxTokens,
    Other(String),
}

/// One complete model reply.
#[derive(Clone, Debug, PartialEq)]
pub struct AssistantTurn {
    pub blocks: Vec<Block>,
    pub stop: Stop,
    pub usage: Usage,
}

/// A tool the model may call.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    /// JSON schema of the arguments.
    pub schema: Value,
}

/// Something the UI can show while a reply streams in.
#[derive(Clone, Debug, PartialEq)]
pub enum Delta {
    Text(String),
    Thinking(String),
    /// The model began a tool call (its arguments are still streaming).
    ToolStart {
        name: String,
    },
}

/// Cut `s` to at most `max` bytes at a character boundary, marking the cut.
pub fn truncate(s: &str, max: usize, note: &str) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[{note}: {} of {} bytes shown]", &s[..end], end, s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_respects_characters() {
        let s = "ä".repeat(10);
        for max in 0..25 {
            let t = truncate(&s, max, "cut");
            assert!(t.is_char_boundary(t.len()));
            if max >= s.len() {
                assert_eq!(t, s);
            }
        }
        assert!(truncate("abcdef", 3, "cut").starts_with("abc\n[cut"));
    }

    #[test]
    fn usage_saturates() {
        let mut u = Usage { input: u64::MAX, ..Usage::default() };
        u.add(&Usage { input: 5, output: 2, ..Usage::default() });
        assert_eq!(u.input, u64::MAX);
        assert_eq!(u.output, 2);
    }
}
