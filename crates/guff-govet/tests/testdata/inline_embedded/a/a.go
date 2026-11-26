package a

type Val int

//go:fix inline
type Value = Val

// Embedding the alias would rename the field (Value -> Val): not inlined,
// whether embedded plainly or by pointer.
type S struct {
	Value
}

type P struct {
	*Value
}

// x/tools issue78994: a parameter or result without a name is not an
// embedded field.
func f(Value) Value { return 1 }

var x []Value
