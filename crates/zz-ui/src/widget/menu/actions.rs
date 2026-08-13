//! Keyboard actions for the popup menu.

use gpui::{Action, actions};
use serde::Deserialize;

/// Accept the highlighted item. `secondary` marks the alternate accept,
/// cmd/ctrl-enter.
#[derive(Clone, Action, PartialEq, Eq, Deserialize)]
#[action(namespace = zz_menu, no_json)]
pub struct Confirm {
    pub secondary: bool,
}

actions!(
    zz_menu,
    [Cancel, SelectUp, SelectDown, SelectLeft, SelectRight]
);
