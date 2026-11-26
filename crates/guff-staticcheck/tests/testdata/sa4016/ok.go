package main

// A named constant that is zero for some reason other than being written
// `= iota`. Upstream's ident branch requires the spec's value to be literally
// the identifier `iota`, and its literal branch requires a literal, so neither
// fires. syncthing's lib/fs writes this — eight option flags copied from `os`,
// one of which (`O_RDONLY`) is 0 — and guff reported the whole `|` chain.
const (
	sysReadOnly = 0x0
	sysAppend   = 0x400
)

const (
	OptAppend   = sysAppend
	OptReadOnly = sysReadOnly
)

func namedZeroConst(x int) int {
	return x | OptAppend | OptReadOnly
}

// A spec that declares more than one name is declined even when it does say
// `iota` ("TODO(dh): we could support this").
const (
	pairA, pairB = iota, iota
)

func multiNameSpec(x int) int { return x | pairA }

// A constant that *is* written `= iota` but is not the zero one.
const (
	stepZero = iota
	stepOne
)

// Non-zero operands, and a non-integer left operand.
func nonZero(x int, s string) (int, int, string) {
	return x | stepOne, x & 1, s + ""
}

func main() {
	_ = namedZeroConst(1)
	_ = multiNameSpec(1)
	_, _, _ = nonZero(1, "")
	_ = pairB
	_ = stepZero
}
