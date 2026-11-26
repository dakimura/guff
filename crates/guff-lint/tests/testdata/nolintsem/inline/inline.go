// Package inline covers same-line `//nolint` directives.
//
// Every `mkerr()` call below is an errcheck finding; what differs is the
// directive attached to it. Upstream reference: golangci-lint 2.12.2
// pkg/result/processors/nolint_filter.go (suppression) and
// pkg/golinters/nolintlint/internal/nolintlint.go (the directive's own
// diagnostics, which fire even when the directive works).
package inline

func mkerr() error { return nil }

func Specific() {
	mkerr() //nolint:errcheck // suppressed, with an explanation
}

func Bare() {
	mkerr() //nolint
}

func All() {
	mkerr() //nolint:all
}

func List() {
	// ineffassign is named but has nothing to suppress here: upstream reports
	// the per-linter unused candidate for it and not for errcheck.
	mkerr() //nolint:errcheck,ineffassign
}

func LeadingSpace() {
	// `strings.TrimLeft(text, "/ ")` in the filter means this still suppresses,
	// while nolintlint reports it as not machine-readable.
	mkerr() // nolint:errcheck
}

func UpperCase() {
	mkerr() //nolint:ErrCheck
}

func Unknown() {
	mkerr() //nolint:doesnotexist
}

func NotADirective() {
	mkerr() //nolintfoo
}

func SpaceBeforeColon() {
	mkerr() //nolint :errcheck
}

func Ineffectual() int {
	x := 1 //nolint:ineffassign
	x = 2
	return x
}

func IneffectualDirectiveOnTheWrongLine() int {
	y := 1
	y = 2 //nolint:ineffassign
	return y
}
