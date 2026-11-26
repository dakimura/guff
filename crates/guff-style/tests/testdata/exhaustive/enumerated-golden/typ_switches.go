package typ

// guff-written companion to exhaustive v0.15.0's enumerated testdata
// (enum/typ): upstream asserts each type's member set as an object fact,
// which golangci-lint does not print. An empty switch over each type spells
// the same set out in a "missing cases" message instead — or prints nothing
// when the type is not enumerated at all.

func _(v t1) {
	switch v {
	}
}

func _(v t2) {
	switch v {
	}
}

func _(v t3) {
	switch v {
	}
}

func _(v M1) {
	switch v {
	}
}

func _(v t11) {
	switch v {
	}
}

// An alias of an enumerated type: the switch is over t11.
func _(v A2) {
	switch v {
	}
}

func _(v FileMode) {
	switch v {
	}
}

// Permitted types without constants: not enumerated.
func _(v r1, w p1, x n1) {
	switch v {
	}
	switch w {
	}
	switch x {
	}
}

// A function-local enumerated type over complex128.
func _() {
	type c complex128
	const (
		c0 c = 1i
		c1 c = 2i
		c2   = c1
	)
	var v c
	switch v {
	}
	switch v {
	case c0:
	}
}

// A constant declared with an alias of an enumerated type is not a member.
type al = t3

const y9 al = 90

func _(v al) {
	switch v {
	case y1:
	}
}

// Generic types are not enumerated; an instance's constant is not a member.
type g[E any] int

const g0 g[int] = 0

func _(v g[int]) {
	switch v {
	}
}
