//! SolidCraft's in-app AI assistant, without any UI toolkit.
//!
//! The assistant models through the same tool surface as the MCP server
//! ([`solidcraft_mcp::tool_definitions`]), so there is one way to change a design: engine
//! commands. This crate holds what is independent of a window: settings, the URL policy, the
//! provider protocols, the tool loop, the agent bridge and the conversation model. The desktop
//! app supplies the HTTP transport ([`net::HttpStream`]) and the UI shows the result.
//!
//! Nothing here may panic: model output, server replies and agent messages are all untrusted
//! input.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod acp;
pub mod context;
pub mod markdown;
pub mod model;
pub mod net;
pub mod policy;
pub mod printmode;
pub mod provider;
pub mod runtime;
pub mod settings;
pub mod slash;
pub mod sse;
pub mod store;
pub mod tool_loop;
pub mod tools;
