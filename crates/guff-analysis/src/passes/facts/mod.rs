//! Analysis fact producers.

pub mod ctrlflow;
pub mod deprecated;
pub mod generated;
pub mod purity;

pub use ctrlflow::analyzer as ctrlflow_analyzer;
pub use ctrlflow::{CtrlFlowResult, NoReturn};
pub use deprecated::analyzer as deprecated_analyzer;
pub use generated::analyzer as generated_analyzer;
pub use generated::{GeneratedResult, Generator};
pub use purity::analyzer as purity_analyzer;
pub use purity::{IsPure, PurityResult};
