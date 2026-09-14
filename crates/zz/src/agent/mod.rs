//! The Agent pane feature, behind the `agent-pane` cargo feature.

pub(crate) mod attachment;

#[cfg(feature = "agent-pane")]
pub(crate) mod controller;
/// Unconditional: the workspace chrome renders agent badges in every build,
/// and without the feature the status map is simply always empty.
pub(crate) mod sound;
#[cfg(feature = "agent-pane")]
mod view;

#[cfg(feature = "agent-pane")]
pub use controller::AgentController;
#[cfg(feature = "agent-pane")]
pub(crate) use controller::AgentControllerEvent;
#[cfg(feature = "agent-pane")]
pub(crate) use view::AgentView;
#[cfg(feature = "agent-pane")]
pub use zz_config::agent_preferences::AgentPreferences;

#[cfg(not(feature = "agent-pane"))]
mod stub;
#[cfg(not(feature = "agent-pane"))]
pub use stub::{AgentController, AgentPreferences};
#[cfg(not(feature = "agent-pane"))]
pub(crate) use stub::{AgentControllerEvent, AgentView};
