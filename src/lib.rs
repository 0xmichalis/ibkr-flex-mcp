//! Read-only Interactive Brokers Flex Query client and (forthcoming) MCP server.
//!
//! The [`flex`] module is the functional core: a transport-agnostic client that runs the
//! Flex Web Service v3 two-step protocol (SendRequest -> poll GetStatement) and returns the
//! raw statement XML. It is read-only by construction — the Flex Web Service cannot trade.

pub mod flex;
pub mod mcp;
