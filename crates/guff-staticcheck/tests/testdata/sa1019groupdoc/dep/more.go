package dep

// Deprecated: whole group is gone.
const (
	GroupA = 1
	GroupB = 2
)

// Deprecated: group message.
const (
	MixA = 1
	// Deprecated: spec message.
	MixB = 2
	MixC = 3
)

const (
	// Deprecated: pair message.
	PairA, PairB = 1, 2
	PairC        = 3
)

// Deprecated: type group message.
type (
	TypeA struct{}
	TypeB struct{}
)

const (
	LineA = 1 // Deprecated: trailing comment, not a doc.
	LineB = 2
)

const (
	// Some prose first.
	//
	// Deprecated: second paragraph.
	ParaA = 1
	ParaB = 2
)

type Fields struct {
	Plain int
	// Deprecated: field message.
	Old int
	Also int
}

type Iface interface {
	Plain()
	// Deprecated: method message.
	Old()
	Also()
}
