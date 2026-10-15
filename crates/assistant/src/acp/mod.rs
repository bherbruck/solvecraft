//! Driving external agent programs (Claude Code, OpenCode, Gemini CLI, Codex, …) over the Agent
//! Client Protocol: JSON-RPC 2.0, one message per line, on the program's stdin and stdout.
//!
//! The agent models through SolidCraft's MCP server, which it starts itself from the
//! `mcpServers` we hand it in `session/new`; that server connects back to the app's agent
//! endpoint (see `docs/assistant.md`). We advertise no file-system or terminal capability, so a
//! well-behaved agent never asks us for either, and anything else it asks for is refused.

pub mod presets;
pub mod process;
pub mod protocol;
pub mod session;
