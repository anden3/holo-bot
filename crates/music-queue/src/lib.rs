mod errors;
mod event_handlers;
mod macros;
mod parameter_types;
mod prelude;
mod queue;
mod wrapper;

pub mod events;
pub mod metadata;

pub use errors::Error;
pub use parameter_types::*;
pub use prelude::Result;
pub use queue::Queue;
pub use wrapper::MusicData;
