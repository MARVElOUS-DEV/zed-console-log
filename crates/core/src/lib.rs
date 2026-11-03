#![forbid(unsafe_code)]

pub mod actions;
pub mod config;
pub mod format;
pub mod text;

pub use actions::{Action, code_actions};
pub use config::Settings;
pub use text::{Edit, Position};
