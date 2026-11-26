package p

const (
	A = iota
	B = "mixed"
	C = iota
)

// iotamixing reports the block, so each const block that mixes iota with an
// explicit value is its own finding.
const (
	D = iota
	E
	F = 42
)

const (
	G = "first"
	H = iota
)
