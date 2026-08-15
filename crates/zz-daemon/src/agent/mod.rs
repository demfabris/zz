//! Daemon-owned agent runtime: adapter children, the session journal, and the
//! stream the clients replay and tail. See knowledge/designs/agent-daemon-runtime.md.
//!
//! [`host::AgentHost`] is the only entry point: it opens panes, owns one thread
//! per pane, and hands every item to the sink its caller supplied. The wiring
//! phase of the campaign (daemon-side subscribe and fan-out) is that caller —
//! until it lands, the module is reachable only from its own tests, which is
//! why dead code is tolerated here and nowhere else.
#![allow(dead_code)]

pub(crate) mod environment;
pub(crate) mod host;
pub(crate) mod journal;
pub(crate) mod paths;
pub(crate) mod profile;
pub(crate) mod runtime;
pub(crate) mod stream;
pub(crate) mod turn_snapshot;
