// Package good builds, and every //nolint directive in it covers a finding
// that the enabled linters really do produce. Upstream reports nothing here:
// the findings are suppressed by the directives, and the directives are
// therefore used.
//
// It exists to prove that the *other* package failing to build does not turn
// these into `directive ... is unused`. It used to: nolintlint's findings are
// born after the typecheck-overrides-everything filter has already run, so
// they walked past it while the findings they were meant to be compared
// against had been deleted.
package good

import "os"

var backends []int

func init() { //nolint:gochecknoinits
	backends = append(backends, 1)
}

// Write reports nothing under errcheck because the directive covers it, and
// nothing under nolintlint because the directive is used.
func Write() {
	os.Stdout.WriteString("x") //nolint:errcheck
}
