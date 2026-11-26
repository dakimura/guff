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

// Wrapper embeds Options by value, so `Options.Old` is *promoted* onto it. The
// field's `Deprecated:` doc still lives on `Options`, and that is how the
// importer's source scan keys it — a lookup keyed by the selection's receiver
// asked for `Wrapper.Old`, which nothing ever writes, and stayed silent.
type Wrapper struct {
	Options
	Extra int
}

// PtrWrapper embeds *Options: same promotion, one pointer hop.
type PtrWrapper struct {
	*Options
}

// Holder names a Wrapper as an ordinary field. buildkit's shape is exactly this
// — `d.image.Config.ArgsEscaped`, a named field selection and then a promoted
// one.
type Holder struct {
	Cfg Wrapper
}

// Other declares its own `Old`, which is NOT deprecated. A lookup keyed by
// field name alone would report it.
type Other struct {
	Old int
}
