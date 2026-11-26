package dep

// The deprecations SA1019 has to tell apart. Nothing here is imported by the
// checker directly: the messages reach it as facts exported while this package
// is analysed, and the shapes below are the ones the group/spec split gets
// wrong in different directions -- a doc on one spec, a doc on the group, both
// at once, a multi-name spec, a type group, a trailing line comment (not a
// doc) and a message in a second paragraph.

type Kind int32

const (
	KindA Kind = 0
	KindB Kind = 1
	// Deprecated: Marked as deprecated in x.proto.
	KindC Kind = 2
	// Deprecated: Marked as deprecated in x.proto.
	KindD Kind = 3
	KindE Kind = 4
)

// Deprecated: use NewThing.
func OldThing() {}

func NewThing() {}

type (
	Alpha struct{}
	// Deprecated: use Alpha.
	Beta  struct{}
	Gamma struct{}
)

var (
	VarA = 1
	// Deprecated: use VarA.
	VarB = 2
	VarC = 3
)
