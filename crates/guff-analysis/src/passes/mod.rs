//! Built-in analysis passes.

pub mod buildir;
pub mod facts;
pub mod inspect;
pub mod printast;
pub mod printf;

pub use buildir::analyzer as buildir_analyzer;
pub use inspect::analyzer as inspect_analyzer;
pub use printast::analyzer as printast_analyzer;
pub use printf::analyzer as printf_analyzer;
