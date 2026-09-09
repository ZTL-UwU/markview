//! Desktop application and platform services.
pub use markview_core::{document, layout};
pub use markview_render as render;
pub mod app;
mod benchmark;
mod cli;
mod file;
mod platform;
mod settings;
mod state;
mod watch;
mod worker;
