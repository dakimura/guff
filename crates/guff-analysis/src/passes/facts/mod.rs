//! Analysis fact producers.

pub mod deprecated;
pub mod generated;

pub use deprecated::analyzer as deprecated_analyzer;
pub use generated::analyzer as generated_analyzer;
pub use generated::{GeneratedResult, Generator};
