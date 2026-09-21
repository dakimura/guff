package modernize

import "reflect"

type MyStruct struct{ N int }

type Expr interface {
	String() string
}

func typeofVar() reflect.Type {
	var zero MyStruct
	return reflect.TypeOf(zero)
}

func typeofElem() reflect.Type {
	return reflect.TypeOf((*MyStruct)(nil)).Elem()
}

// Interface-typed args are dynamic; must not suggest TypeFor.
func typeofIface(expr Expr) reflect.Type {
	return reflect.TypeOf(expr)
}

type AVeryLongTypeNameIndeed struct{ N int }

type holder struct{ s string }

func effectful() string { return "" }

func hold() holder { return holder{} }

// `*new(T)` is upstream's own spelling for a zero value, and `new` is one of
// the pure builtins `NoEffects` lets through. beats' cassandra marshaller
// writes eleven of these in a row.
func starNew() []reflect.Type {
	return []reflect.Type{
		reflect.TypeOf(*new(string)),
		reflect.TypeOf(*new(int64)),
		reflect.TypeOf(*new([]byte)),
	}
}

// The rest of `CallsPureBuiltin`: len, cap, complex, imag, real, make, new,
// max, min — and not append, clear, close, copy, delete, panic, print,
// println or recover.
func pureBuiltins(s []int) []reflect.Type {
	return []reflect.Type{
		reflect.TypeOf(len(s)),
		reflect.TypeOf(make([]int, 3)),
		reflect.TypeOf(min(1, 2)),
	}
}

// Operand shapes `NoEffects` accepts that an outermost-node-only check missed.
func operandShapes(a, b int, s []int, x any, m map[string]int, k string) []reflect.Type {
	return []reflect.Type{
		reflect.TypeOf(a + b),
		reflect.TypeOf(s[1:2]),
		reflect.TypeOf(x.(MyStruct)),
		reflect.TypeOf(m[k]),
	}
}

// Effects one level down. `ast.Inspect` keeps walking, so a selector over a
// call and a composite literal holding one are both out.
func effects() []reflect.Type {
	return []reflect.Type{
		reflect.TypeOf(effectful()),
		reflect.TypeOf(hold().s),
		reflect.TypeOf(holder{s: effectful()}),
	}
}

// Too long to spell: `newLen >= 16 && newLen > 3*oldLen`. The operand is one
// character; only the second of these is worth rewriting.
func elemTooLong(p *AVeryLongTypeNameIndeed, q *MyStruct) []reflect.Type {
	return []reflect.Type{
		reflect.TypeOf(p).Elem(),
		reflect.TypeOf(q).Elem(),
	}
}

// `TypeOf((*T)(nil))` standing alone, with no `.Elem()` after it.
func nilConversionWithoutElem() reflect.Type {
	return reflect.TypeOf((*MyStruct)(nil))
}

type List[T any] struct{ items []T }

// A conversion of nil is not an untyped nil, so `Types[expr].IsNil()` is false
// and these are ordinary operands. `any` is an *alias*, and upstream tests
// `NamedOrAlias` before it looks at what a type stands for — so `map[string]any`
// is not "complicated" the way an unnamed `interface{}` would be.
func nilConversions() []reflect.Type {
	return []reflect.Type{
		reflect.TypeOf(map[string]any(nil)),
		reflect.TypeOf([]int(nil)),
		reflect.TypeOf(map[string]int(nil)),
	}
}

// The `NamedOrAlias` arm is not a dead end: upstream descends into the type
// arguments, so a named generic instantiated with an unnamed struct or an
// unnamed interface is still complicated.
func generics() []reflect.Type {
	var a List[int]
	var b List[struct{ N int }]
	var c List[Expr]
	var d List[interface{ Bar() }]
	return []reflect.Type{
		reflect.TypeOf(a),
		reflect.TypeOf(b),
		reflect.TypeOf(c),
		reflect.TypeOf(d),
	}
}
