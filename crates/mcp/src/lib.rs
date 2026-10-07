//! SolveCraft MCP server.
//!
//! JSON-RPC 2.0 over stdio (newline-delimited), implementing the MCP lifecycle, tools and
//! resources. Tools map onto the control-channel methods (`engine.execute`, `document.inspect`,
//! `ui.render`…, see `docs/control-protocol.md`), served either by an in-process headless
//! [`Headless`] session or by a running app through its loopback control port ([`Remote`]), so a
//! person can watch the agent model. Every modelling step is an engine command; tools never open
//! dialogs.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod backend;
mod server;
mod tools;

pub use backend::{Backend, Headless, Remote};
pub use server::{PROTOCOL_VERSION, SUPPORTED_VERSIONS, Server};
pub use tools::{call_tool, tool_definitions};

#[cfg(test)]
mod tests;
