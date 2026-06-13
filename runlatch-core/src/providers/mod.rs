//! Built-in [`AutostartProvider`](crate::provider::AutostartProvider) implementations.

mod systemd;
mod xdg;

pub use systemd::SystemdProvider;
pub use xdg::XdgAutostartProvider;
