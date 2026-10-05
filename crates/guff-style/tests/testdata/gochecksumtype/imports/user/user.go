// Package user switches over sum types declared in the packages it imports:
// one in the main module (ops), one in another module (sumdep).
package user

import (
	"example.com/sumdep"
	"example.com/sumuser/ops"
)

func shapes(s sumdep.Shape) {
	switch s.(type) {
	case sumdep.Circle:
	}

	switch s.(type) {
	case sumdep.Round, *sumdep.Square:
	}

	switch s.(type) {
	default:
	}

	switch v := s.(type) {
	case *sumdep.Square:
		_ = v
	default:
		panic(v)
	}
}

// A defined type over the same interface is the same sum type to findDef
// (`types.Identical(needle.Underlying(), def.Ty)`).
type Local sumdep.Shape

func local(l Local) {
	switch l.(type) {
	case *sumdep.Square:
	}
}

func events(e sumdep.Event) {
	switch e.(type) {
	case sumdep.Click:
	}
}

func open(o sumdep.Open) {
	switch o.(type) {
	}
}

func opsSwitch(o ops.Op) {
	switch o.(type) {
	case ops.Add:
	}

	switch o.(type) {
	case ops.Add, *ops.Sub, ops.Neg:
	}

	switch o.(type) {
	case ops.Unary:
	}
}

func nested() {
	//sumtype:decl
	type Inner interface{ inner() }

	var i Inner
	switch i.(type) {
	}
}
