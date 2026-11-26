//! [`LoadMode`] controls the amount of detail returned when loading packages.
//!
//! Port of `packages.LoadMode` from `packages.go`.

/// Controls which [`super::Package`] fields are populated by [`super::load`].
///
/// The zero value is equivalent to [`LoadMode::LOAD_FILES`].
///
/// Equivalent to `packages.LoadMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct LoadMode(pub u32);

impl LoadMode {
    /// Adds `Name` and `PkgPath`.
    pub const NEED_NAME: Self = Self(1 << 0);
    /// Adds `Dir`, `GoFiles`, `OtherFiles`, and `IgnoredFiles`.
    pub const NEED_FILES: Self = Self(1 << 1);
    /// Adds `CompiledGoFiles`.
    pub const NEED_COMPILED_GO_FILES: Self = Self(1 << 2);
    /// Adds `Imports`.
    pub const NEED_IMPORTS: Self = Self(1 << 3);
    /// Recursively loads fields requested by the mode for imported packages.
    pub const NEED_DEPS: Self = Self(1 << 4);
    /// Adds `ExportFile`.
    pub const NEED_EXPORT_FILE: Self = Self(1 << 5);
    /// Adds `Types`, `Fset`, and `IllTyped` (Phase 4).
    pub const NEED_TYPES: Self = Self(1 << 6);
    /// Adds `Syntax` and `Fset` (Phase 4).
    pub const NEED_SYNTAX: Self = Self(1 << 7);
    /// Adds `TypesInfo` and `Fset` (Phase 4).
    pub const NEED_TYPES_INFO: Self = Self(1 << 8);
    /// Adds `TypesSizes`.
    pub const NEED_TYPES_SIZES: Self = Self(1 << 9);
    /// Adds `ForTest` (when `Config.tests` is set).
    pub const NEED_FOR_TEST: Self = Self(1 << 11);
    /// Adds `Module`.
    pub const NEED_MODULE: Self = Self(1 << 14);
    /// Adds `EmbedFiles`.
    pub const NEED_EMBED_FILES: Self = Self(1 << 15);
    /// Adds `EmbedPatterns`.
    pub const NEED_EMBED_PATTERNS: Self = Self(1 << 16);
    /// Adds `Target`.
    pub const NEED_TARGET: Self = Self(1 << 17);

    /// Lists of files in each package.
    pub const LOAD_FILES: Self = Self(
        Self::NEED_NAME.0 | Self::NEED_FILES.0 | Self::NEED_COMPILED_GO_FILES.0,
    );
    /// [`LOAD_FILES`] plus imports.
    pub const LOAD_IMPORTS: Self = Self(Self::LOAD_FILES.0 | Self::NEED_IMPORTS.0);
    /// Exported type information for initial packages.
    pub const LOAD_TYPES: Self = Self(
        Self::LOAD_IMPORTS.0 | Self::NEED_TYPES.0 | Self::NEED_TYPES_SIZES.0,
    );
    /// Typed syntax for initial packages.
    pub const LOAD_SYNTAX: Self = Self(
        Self::LOAD_TYPES.0 | Self::NEED_SYNTAX.0 | Self::NEED_TYPES_INFO.0,
    );
    /// Typed syntax for initial packages and all dependencies.
    pub const LOAD_ALL_SYNTAX: Self = Self(Self::LOAD_SYNTAX.0 | Self::NEED_DEPS.0);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Returns the union of several load modes (e.g. multiple linter configs).
    pub fn union_all(modes: &[Self]) -> Self {
        modes.iter().copied().fold(Self::empty(), |acc, m| acc.union(m))
    }

    /// Expands mode flags with implied dependencies, matching Go's `impliedLoadMode`.
    pub fn implied(self) -> Self {
        let mut mode = self;
        if mode.contains(Self::NEED_DEPS)
            || mode.contains(Self::NEED_TYPES)
            || mode.contains(Self::NEED_TYPES_INFO)
        {
            mode = mode.union(Self::NEED_IMPORTS);
        }
        if mode.contains(Self::NEED_TYPES) {
            mode = mode.union(Self::NEED_MODULE);
        }
        mode
    }

    /// Normalizes the zero value to [`LOAD_FILES`], matching Go's `newLoader`.
    pub fn normalize(self) -> Self {
        if self == Self::empty() {
            Self::LOAD_FILES
        } else {
            self
        }
    }
}

impl std::ops::BitOr for LoadMode {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        self.union(rhs)
    }
}

impl std::ops::BitOrAssign for LoadMode {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

impl std::ops::BitAnd for LoadMode {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_value_equals_load_files() {
        assert_eq!(LoadMode::default().normalize(), LoadMode::LOAD_FILES);
    }

    #[test]
    fn load_files_contains_name_files_compiled() {
        let mode = LoadMode::LOAD_FILES;
        assert!(mode.contains(LoadMode::NEED_NAME));
        assert!(mode.contains(LoadMode::NEED_FILES));
        assert!(mode.contains(LoadMode::NEED_COMPILED_GO_FILES));
        assert!(!mode.contains(LoadMode::NEED_TYPES));
    }

    #[test]
    fn union_combines_flags() {
        let a = LoadMode::NEED_NAME | LoadMode::NEED_FILES;
        let b = LoadMode::NEED_TYPES | LoadMode::NEED_SYNTAX;
        let u = LoadMode::union_all(&[a, b]);
        assert!(u.contains(LoadMode::NEED_NAME));
        assert!(u.contains(LoadMode::NEED_FILES));
        assert!(u.contains(LoadMode::NEED_TYPES));
        assert!(u.contains(LoadMode::NEED_SYNTAX));
    }

    #[test]
    fn implied_adds_imports_for_types() {
        let mode = LoadMode::NEED_TYPES.implied();
        assert!(mode.contains(LoadMode::NEED_IMPORTS));
        assert!(mode.contains(LoadMode::NEED_MODULE));
    }

    #[test]
    fn load_syntax_preset() {
        let mode = LoadMode::LOAD_SYNTAX;
        assert!(mode.contains(LoadMode::NEED_TYPES_INFO));
        assert!(mode.contains(LoadMode::NEED_SYNTAX));
    }
}
