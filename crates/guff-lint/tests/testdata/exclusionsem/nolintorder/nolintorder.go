// Package nolintorder exists for the order of two processors: every exclusion
// runs *before* the `//nolint` one upstream, so a finding an exclusion removes
// never reaches a directive, and the directive is left unused.
package nolintorder

type tx struct{}

func (t *tx) Rollback() error { return nil }

func (t *tx) Commit() error { return nil }

type file struct{}

func (f *file) Close() error { return nil }

func (f *file) Sync() error { return nil }

// The `source:` rule removes this one, so the directive is unused.
func excludedByRule(t *tx) {
	defer t.Rollback() //nolint:errcheck
}

// EXC0001 of the std-error-handling preset covers `.*Close`, so this directive
// is unused too — the presets are an exclusion like any other.
func excludedByPreset(f *file) {
	defer f.Close() //nolint:errcheck
}

// Nothing excludes these, so the directives really are used.
func onlyNolinted(t *tx, f *file) {
	defer t.Commit() //nolint:errcheck
	defer f.Sync()   //nolint:errcheck
}

// Excluded with no directive at all: silent, and nothing to report as unused.
func onlyExcluded(t *tx, f *file) {
	defer t.Rollback()
	defer f.Close()
}

// Neither: an ordinary errcheck finding.
func neither(t *tx, f *file) {
	defer t.Commit()
	defer f.Sync()
}
