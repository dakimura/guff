//! Utilities for constructing and traversing SSA programs — port of
//! `golang.org/x/tools/go/ssa/ssautil`.

pub mod load;
pub mod switch;
pub mod visit;

pub use load::{
    all_packages, build_package_for_analysis, build_package_from_loaded, build_package_from_source, packages,
    BuildFromLoadedError, BuildPackageResult, LoadedPackage,
};
pub use switch::{switches, ConstCase, Switch, TypeCase};
pub use visit::{all_functions, main_packages};
