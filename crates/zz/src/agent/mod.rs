//! The Agent pane feature, behind the `agent-pane` cargo feature.

pub(crate) mod attachment;

#[cfg(feature = "agent-pane")]
pub(crate) mod controller;
#[cfg(feature = "agent-pane")]
mod environment;
#[cfg(feature = "agent-pane")]
mod journal;
#[cfg(feature = "agent-pane")]
mod preferences;
#[cfg(feature = "agent-pane")]
mod profile;
#[cfg(feature = "agent-pane")]
mod turn_snapshot;
#[cfg(feature = "agent-pane")]
mod view;

#[cfg(feature = "agent-pane")]
pub use controller::AgentController;
#[cfg(feature = "agent-pane")]
pub(crate) use controller::{AgentAttention, AgentControllerEvent};
#[cfg(feature = "agent-pane")]
pub use environment::warm_agent_adapter_cache;
#[cfg(feature = "agent-pane")]
pub use preferences::AgentPreferences;
#[cfg(feature = "agent-pane")]
pub(crate) use view::AgentView;

#[cfg(not(feature = "agent-pane"))]
mod stub;
#[cfg(not(feature = "agent-pane"))]
pub(crate) use stub::{AgentAttention, AgentControllerEvent, AgentView};
#[cfg(not(feature = "agent-pane"))]
pub use stub::{AgentController, AgentPreferences, warm_agent_adapter_cache};
