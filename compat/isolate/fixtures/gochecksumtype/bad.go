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
