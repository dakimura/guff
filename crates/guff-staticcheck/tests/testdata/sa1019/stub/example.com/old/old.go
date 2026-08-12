package old

// Deprecated: call New.
func Legacy() {}

// Deprecated: use NewClient instead.
type OldClient interface {
	Do()
}

// Options is not deprecated; one of its fields is. A field's `Deprecated:` doc
// lives inside the struct type, which the importer's source scan did not walk —
// so a deprecated field was silent for every importing package, and every
// `//nolint:staticcheck` above one was reported unused instead.
type Options struct {
	Fine int

	// Deprecated: use Fine instead.
	Old int
}
