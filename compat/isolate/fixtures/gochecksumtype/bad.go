package p

//sumtype:decl
type S interface{ isS() }

type A struct{}
func (A) isS() {}

type B struct{}
func (B) isS() {}

func Bad(x S) {
	switch x.(type) {
	case A:
	}
}

// go-check-sumtype has more than the missing-case message: a default clause and
// a nil-case omission are separate sentences.

//sumtype:decl
type T interface{ isT() }

type X struct{}

func (X) isT() {}

type Y struct{}

func (Y) isT() {}

func MissingBoth(v T) {
	switch v.(type) {
	}
}

func OnlyDefault(v T) {
	switch v.(type) {
	default:
	}
}
