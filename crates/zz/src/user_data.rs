//! Location and permission policy for user-owned application data.
//!
//! The policy lives in `zz-daemon` because the daemon's agent journal answers
//! to it too; this is the GUI's view of the same module.

pub(crate) use zz_daemon::user_data::{
    platform_data_dir, restrict_directory_to_current_user, restrict_to_current_user,
};
