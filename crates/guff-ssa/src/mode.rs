use std::fmt;

/// `BuilderMode` is a bitmask of options for diagnostics and checking.
///
/// Mirrors go/ssa's `BuilderMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BuilderMode(u32);

impl BuilderMode {
    /// Print package inventory to stdout. (Go: `PrintPackages`)
    pub const PRINT_PACKAGES: Self = Self(1 << 0);
    /// Print function SSA code to stdout. (Go: `PrintFunctions`)
    pub const PRINT_FUNCTIONS: Self = Self(1 << 1);
    /// Log source locations as SSA builder progresses. (Go: `LogSource`)
    pub const LOG_SOURCE: Self = Self(1 << 2);
    /// Perform sanity checking of function bodies. (Go: `SanityCheckFunctions`)
    pub const SANITY_CHECK_FUNCTIONS: Self = Self(1 << 3);
    /// Build naïve SSA form: don't replace local loads/stores with registers.
    /// (Go: `NaiveForm`)
    pub const NAIVE_FORM: Self = Self(1 << 4);
    /// Build packages serially, not in parallel. (Go: `BuildSerially`)
    pub const BUILD_SERIALLY: Self = Self(1 << 5);
    /// Enable debug info for all packages. (Go: `GlobalDebug`)
    pub const GLOBAL_DEBUG: Self = Self(1 << 6);
    /// Build init functions without guards or calls to dependent inits.
    /// (Go: `BareInits`)
    pub const BARE_INITS: Self = Self(1 << 7);
    /// Instantiate generics functions (monomorphize) while building.
    /// (Go: `InstantiateGenerics`)
    pub const INSTANTIATE_GENERICS: Self = Self(1 << 8);

    /// Returns `true` if `self` contains all of the flags in `other`.
    pub fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Set parses the flag characters in `s` and updates `self`.
    ///
    /// The string is a sequence of zero or more of these letters:
    /// - `C`: perform sanity checking of the SSA form.
    /// - `D`: include debug info for every function.
    /// - `P`: print package inventory.
    /// - `F`: print function SSA code.
    /// - `S`: log source locations as SSA builder progresses.
    /// - `L`: build distinct packages serially instead of in parallel.
    /// - `N`: build naive SSA form.
    /// - `I`: build bare init functions.
    /// - `G`: instantiate generic function bodies.
    ///
    /// (Go: `(*BuilderMode).Set`)
    pub fn set(&mut self, s: &str) -> Result<(), String> {
        let mut mode = 0;
        for c in s.chars() {
            match c {
                'D' => mode |= Self::GLOBAL_DEBUG.0,
                'P' => mode |= Self::PRINT_PACKAGES.0,
                'F' => mode |= Self::PRINT_FUNCTIONS.0,
                'S' => mode |= Self::LOG_SOURCE.0 | Self::BUILD_SERIALLY.0,
                'C' => mode |= Self::SANITY_CHECK_FUNCTIONS.0,
                'N' => mode |= Self::NAIVE_FORM.0,
                'L' => mode |= Self::BUILD_SERIALLY.0,
                'I' => mode |= Self::BARE_INITS.0,
                'G' => mode |= Self::INSTANTIATE_GENERICS.0,
                _ => return Err(format!("unknown BuilderMode option: {:?}", c)),
            }
        }
        self.0 = mode;
        Ok(())
    }
}

impl std::ops::BitOr for BuilderMode {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for BuilderMode {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl fmt::Display for BuilderMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Go's String() implementation order:
        if self.contains(Self::GLOBAL_DEBUG) {
            f.write_str("D")?;
        }
        if self.contains(Self::PRINT_PACKAGES) {
            f.write_str("P")?;
        }
        if self.contains(Self::PRINT_FUNCTIONS) {
            f.write_str("F")?;
        }
        if self.contains(Self::LOG_SOURCE) {
            f.write_str("S")?;
        }
        if self.contains(Self::SANITY_CHECK_FUNCTIONS) {
            f.write_str("C")?;
        }
        if self.contains(Self::NAIVE_FORM) {
            f.write_str("N")?;
        }
        if self.contains(Self::BUILD_SERIALLY) {
            f.write_str("L")?;
        }
        if self.contains(Self::BARE_INITS) {
            f.write_str("I")?;
        }
        if self.contains(Self::INSTANTIATE_GENERICS) {
            f.write_str("G")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_mode_set_string() {
        let mut mode = BuilderMode::default();
        mode.set("CP").unwrap();
        assert_eq!(mode.to_string(), "PC");
        assert!(mode.contains(BuilderMode::SANITY_CHECK_FUNCTIONS));
        assert!(mode.contains(BuilderMode::PRINT_PACKAGES));
        assert!(!mode.contains(BuilderMode::PRINT_FUNCTIONS));

        mode.set("S").unwrap();
        // 'S' sets LogSource and BuildSerially. 
        // Display order: S comes before L.
        assert_eq!(mode.to_string(), "SL");
        assert!(mode.contains(BuilderMode::LOG_SOURCE));
        assert!(mode.contains(BuilderMode::BUILD_SERIALLY));

        mode.set("DPI").unwrap();
        assert_eq!(mode.to_string(), "DPI");

        assert!(mode.set("X").is_err());
    }

    #[test]
    fn test_builder_mode_bitor() {
        let mode = BuilderMode::PRINT_PACKAGES | BuilderMode::PRINT_FUNCTIONS;
        assert_eq!(mode.to_string(), "PF");
    }
}
