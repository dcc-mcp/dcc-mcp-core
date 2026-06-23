//! Stateless MCP service (ADR-010).
//!
//! In the stateless model, each request is self-contained — the server does
//! not create sessions or maintain per-client state. Clients discover
//! capabilities via `server/discover` and invoke tools via direct JSON-RPC
//! calls, all within a single HTTP request/response cycle (no SSE, no
//! session lifecycle).
//!
//! This module is a Phase 1a skeleton. Phase 1b will implement the full
//! `handle_stateless_mcp` dispatch logic.
