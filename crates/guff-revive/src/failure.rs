//! A single revive rule violation.

/// One lint failure produced by a revive rule.
///
/// Rules build this with `..Failure::default()` so that a new optional field
/// does not have to be spelled out at all ~140 report sites.
#[derive(Debug, Clone, Default)]
pub struct Failure {
    pub rule: &'static str,
    pub pos: u32,
    pub message: String,
    /// Optional per-failure confidence override (e.g. error-strings capitalization).
    pub confidence: Option<f64>,
    /// Column to report instead of the one `pos` resolves to.
    ///
    /// Upstream a rule returns a `lint.FailurePosition`, so it may build a
    /// `token.Position` by hand rather than derive one from a `token.Pos`.
    /// `line-length-limit` and `file-length-limit` do exactly that, and both
    /// hardcode `Column: 0` — a column no offset can produce.
    pub column: Option<u32>,
    /// Replacement text for the whole line(s) the failure covers, without a
    /// trailing newline.
    ///
    /// Upstream's `lint.Failure.ReplacementLine`. golangci-lint turns it into a
    /// single edit spanning from the start of the failure's first line to the
    /// end of its last, so the value is a *line*, not an expression — revive's
    /// rules build it by matching the source text rather than the AST.
    ///
    /// `None` means no suggested fix, which is also what upstream reports when
    /// its regex fails to match the line.
    pub replacement_line: Option<String>,
    /// End of the node the replacement covers, when it is not on `pos`'s line.
    /// Defaults to `pos` — `ReplacementLine` is one line, but the node it
    /// replaces need not be.
    pub replacement_end: Option<u32>,
}

impl Failure {
    /// Build a failure with the default confidence for `rule` (see [`Self::confidence`]).
    pub fn new(rule: &'static str, pos: u32, message: impl Into<String>) -> Self {
        Self {
            rule,
            pos,
            message: message.into(),
            confidence: None,
            column: None,
            replacement_line: None,
            replacement_end: None,
        }
    }

    /// Build a failure reported at an explicit column (see [`Self::column`]).
    pub fn at_column(
        rule: &'static str,
        pos: u32,
        column: u32,
        message: impl Into<String>,
    ) -> Self {
        Self {
            rule,
            pos,
            message: message.into(),
            confidence: None,
            column: Some(column),
            replacement_line: None,
            replacement_end: None,
        }
    }

    /// Build a failure with an explicit confidence (e.g. error-strings capitalization = 0.6).
    pub fn with_confidence(
        rule: &'static str,
        pos: u32,
        message: impl Into<String>,
        confidence: f64,
    ) -> Self {
        Self {
            rule,
            pos,
            message: message.into(),
            confidence: Some(confidence),
            column: None,
            replacement_line: None,
            replacement_end: None,
        }
    }

    /// Confidence level for this failure (revive's default for most rules: 1.0).
    ///
    /// revive writes `Confidence:` at each report site, and golangci drops
    /// anything below `revive.confidence` (default 0.8). Every value below 1.0
    /// in revive v1.15.0's `rule/` is reproduced here; a rule whose sites all
    /// agree is keyed by name alone, one whose sites differ by message.
    /// Anything not listed reports at 1.0, as upstream does.
    ///
    /// Two of these are load-bearing at the default threshold —
    /// `optimize-operands-order` (0.3) and `modifies-parameter` (0.5) never
    /// reach the user — and the rest decide what survives once a config lowers
    /// or raises `confidence`.
    pub fn confidence(&self) -> f64 {
        if let Some(c) = self.confidence {
            return c;
        }
        let has = |s: &str| self.message.contains(s);
        match self.rule {
            // Uniform across the rule's report sites.
            "optimize-operands-order" => 0.3,
            "modifies-parameter" => 0.5,
            "get-return"
            | "increment-decrement"
            | "unconditional-recursion"
            | "unexported-return"
            | "unnecessary-format" => 0.8,
            "context-as-argument"
            | "epoch-naming"
            | "error-naming"
            | "error-return"
            | "if-return"
            | "time-naming" => 0.9,

            // Rules whose sites disagree.
            // The "should be of the form" sites pass their confidence in: it
            // depends on *how* the comment is wrong (0.8 only when the comment
            // does not mention the name at all).
            "exported" if has("stutters") || has("is repetitive") => 0.8,
            "var-declaration" if has("omit type") => 0.8,
            "var-declaration" if has("zero value") => 0.9,
            "datarace" if has("potential datarace: return value") => 0.8,
            "package-comments" if has("package comment is detached") => 0.9,
            "var-naming" if has("don't use underscores in Go names") => 0.9,
            "var-naming" if has("don't use ALL_CAPS in Go names") || has("should be") => 0.8,
            "time-date" if has("appear to be swapped") || has("argument is negative") => 0.5,
            "time-date"
                if has("days") || has("should be between") || has("useless plus sign") =>
            {
                0.8
            }
            // empty-block reports the same text at 0.9 (RangeStmt) and 1
            // (BlockStmt), so its sites pass the value in explicitly.
            _ => 1.0,
        }
    }

    pub fn format(&self) -> String {
        format!("{}: {}", self.rule, self.message)
    }
}
