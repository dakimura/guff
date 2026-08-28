// Package deprecated exercises the two shapes the old symbol collector could
// not see: a parent doc whose specs are all undocumented, and an exported
// method on an unexported receiver.
package deprecated

// deprecated: none of the specs below has a doc of its own, so the only way
// to reach this comment group is through a symbol that has no Doc.
const (
	Alpha = 1
	Beta  = 2
)

// deprecated: reported at *this* line, not at the spec's own doc below.
const (
	// Gamma is a constant.
	Gamma = 3
)

// deprecated: an unexported group is upstream's to skip — the export filter
// runs before the parent doc is collected.
const (
	delta = 4
)

type hidden int

// deprecated: godoc's export rule does not apply here. Unlike require-doc and
// start-with-name, this rule looks at the method name alone.
func (h hidden) Method() {}

// deprecated: a plain exported func, the shape that already worked.
func Exported() {}

// deprecated: unexported, so skipped.
func unexported() {}
