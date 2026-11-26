package main

func main() {
	var p *int
	var i any
	i = p
	_ = i == nil
}

// Every function below is a shape upstream reports: the left operand of the
// comparison flattens (through `Phi` edges that agree) to a `MakeInterface`.

type iface interface{ M() }

type concrete struct{}

func (*concrete) M() {}

func newConcrete() *concrete { return &concrete{} }

// A variable declaration with a concrete initialiser.
func varInit() bool {
	var d iface = &concrete{}
	return d != nil
}

// A plain assignment, unconditional.
func plainAssign() bool {
	var d iface
	d = &concrete{}
	return d != nil
}

// A short variable declaration with a conversion.
func shortDecl() bool {
	d := iface(&concrete{})
	return d != nil
}

// `==` rather than `!=`, so the qualifier is "never".
func eql() bool {
	var d iface = &concrete{}
	return d == nil
}

// A typed nil pointer held in a variable — still a concrete type.
func typedNilPointer() bool {
	var p *concrete
	var d iface
	d = p
	return d == nil
}

// The concrete value comes from a call.
func fromCall() bool {
	var d iface = newConcrete()
	return d != nil
}

// Type parameters: upstream reports only when the constraint has structural
// terms (`typeparams.NormalTerms` is non-empty).

// A union of two basic types.
func typeParamUnion[T int | string](v T) bool {
	var d any = v
	return d != nil
}

// An approximation term.
func typeParamTilde[T ~int](v T) bool {
	var d any = v
	return d != nil
}

// A bare pointer type as the constraint, which the checker wraps in an
// implicit interface holding that one term.
func typeParamPointer[T *concrete](v T) bool {
	var d any = v
	return d != nil
}

type num interface{ ~int | ~float64 }

// A named union constraint.
func typeParamNamedUnion[T num](v T) bool {
	var d any = v
	return d != nil
}

type stringy interface {
	~string
	Len() int
}

// A constraint carrying both a method and a term.
func typeParamMethodAndTerm[T stringy](v T) bool {
	var d any = v
	return d != nil
}

// An inline interface holding a single pointer term.
func typeParamInlineTerm[T interface{ *concrete }](v T) bool {
	var d any = v
	return d != nil
}
