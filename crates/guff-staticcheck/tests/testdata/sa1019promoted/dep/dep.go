package dep

import "example.com/sa1019promoted/inner"

type Base struct {
	// Deprecated: base field message.
	Old string
	New string
}

// Wrapper embeds Base by value: Base.Old is promoted onto Wrapper.
type Wrapper struct {
	Base
	Extra string
}

// PtrWrapper embeds *Base — the same promotion with a pointer hop.
type PtrWrapper struct {
	*Base
}

// Mid embeds a struct declared in a third package.
type Mid struct {
	inner.Deep
}

// Outer embeds Mid, putting inner.Deep's fields two levels up.
type Outer struct {
	Mid
}

// Holder names a Wrapper as an ordinary field. buildkit's shape is exactly
// this: `d.image.Config.ArgsEscaped`, a named field selection and then a
// promoted one.
type Holder struct {
	Cfg Wrapper
}

// Other declares its own Old, which is NOT deprecated: a lookup keyed by bare
// field name would report it.
type Other struct {
	Old string
}
