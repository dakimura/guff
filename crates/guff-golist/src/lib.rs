//! Native Go package lister (PERF_TASKS_V2 §C-3c).
//!
//! Resolves packages from `go.mod` + the module cache + GOROOT without invoking
//! `go list`. Callers in `guff-packages` convert [`ListResponse`] into a
//! `DriverResponse` and fall back to `go list` on [`Bail`].

mod bail;
pub mod embed;
mod escape;
mod list;
mod modcache;
mod modmeta;
mod resolve;
mod vendor;
mod workspace;

pub use bail::{Bail, BailReason};
pub use embed::{resolve_embed, EmbedError};
pub use escape::escape_path;
pub use list::{list_packages, ListConfig, ListModule, ListPackage, ListResponse};
pub use modcache::{default_gomodcache, module_dir, ModCache};
pub use resolve::ResolvedModule;
pub use vendor::{load_vendor_index, parse_modules_txt, VendorIndex, VendorModule};
pub use workspace::{find_workspace_root, load_workspace, Workspace, WorkspaceModule};
