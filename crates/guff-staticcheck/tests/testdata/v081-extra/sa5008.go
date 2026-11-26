package pkg

// json/v2's `embed` is an option of its own, not an unknown one.
type Embed struct {
	Inner struct{ A int } `json:",embed"`
}

// Two terms that cannot carry `,string`: one finding or two?
type TwoBad[T complex64 | complex128] struct {
	F T `json:",string"`
}

// A named interface is a type set with no terms, so the inner walk finds
// nothing to object to.
type Stringer interface{ String() string }

type PtrToIface[T Stringer] struct {
	F *T `json:",string"`
}

type IfaceField struct {
	F Stringer `json:",string"`
	G *bool    `json:",string"`
	H **int    `json:",string"`
}
