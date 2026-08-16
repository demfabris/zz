//! Daemon-owned agent runtime: adapter children, the session journal, and the
//! stream the clients replay and tail. See knowledge/designs/agent-daemon-runtime.md.
//!
//! [`host::AgentHost`] opens panes and owns one thread per pane;
//! [`fanout::AgentRuntime`] is what the daemon holds, pairing that host with
//! the coalescing lane, the replay ring, and the publisher the daemon
//! implements.

pub(crate) mod environment;
pub(crate) mod fanout;
#[cfg(test)]
pub(crate) mod fixture;
pub(crate) mod git_summary;
pub(crate) mod host;
pub(crate) mod journal;
pub(crate) mod paths;
pub(crate) mod runtime;
pub(crate) mod stream;
