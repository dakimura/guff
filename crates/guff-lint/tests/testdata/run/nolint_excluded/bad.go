package nolint_excluded

type tx struct{}

func (t *tx) Rollback() error { return nil }

func (t *tx) Commit() error { return nil }

// An exclusion rule removes this finding, so the directive never sees one.
func excluded(t *tx) {
	defer t.Rollback() //nolint:errcheck
}

// Nothing excludes this one, so the directive really is used.
func used(t *tx) {
	defer t.Commit() //nolint:errcheck
}
