//nolint:revive // TODO(CINT) Fix revive linter
package siblingdirective

// A doc comment that holds only directives still silences the package's
// missing-comment failure: upstream's `checkPackageComment` returns as soon as
// any file has `Doc != nil`, and never looks at what the doc says. A few lines
// earlier `isEmptyDoc` reads `Doc.Text()`, from which go/ast drops directive
// lines, so the same comment reads as empty there — the two answers are
// supposed to differ. datadog-agent's cmd/cluster-agent/klog.go carries
// exactly this line above its `package main`.
//
// Measured against golangci-lint 2.12.2 (revive v1.15.0).
