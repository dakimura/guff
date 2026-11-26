// Package ops declares a sum type in the main module.
package ops

//sumtype:decl
type Op interface{ op() }

type Add struct{}

func (Add) op() {}

type Sub struct{}

func (*Sub) op() {}

// An extension of the sum type is not a case of its own.
type Unary interface {
	Op
	unary()
}

type Neg struct{}

func (Neg) op()    {}
func (Neg) unary() {}
