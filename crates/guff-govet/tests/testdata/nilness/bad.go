package nilness

// Fixture for the `nilness` analyzer (x/tools v0.44.0). Every function below
// is one shape the pass distinguishes; the ones that report nothing are as
// much the point as the ones that do — an analyzer that fired on every nil
// comparison would still satisfy a "contains" assertion.

type T struct{ F int }

func (t *T) M() {}

type I interface{ M() }

func sink(v ...any) {}

// --- degenerate conditions -------------------------------------------------

func tautologicalNilEq() {
	var p *T
	if p == nil {
		sink(p)
	}
}

func tautologicalNonNilNe() {
	q := &T{}
	if q != nil {
		sink(q)
	}
}

func impossibleNonNilEq() {
	q := &T{}
	if q == nil {
		sink(q)
	}
}

func impossibleNilNe(p *T) {
	if p == nil {
		if p != nil {
			sink(p)
		}
	}
}

func nestedTautology(p *T) int {
	if p == nil {
		if p == nil {
			return 1
		}
		return p.F // unreachable: pruned, so not reported
	}
	return 0
}

func funcValueEq() {
	f := impossibleNonNilEq
	if f == nil {
		sink(f)
	}
}

func makeInterfaceEq(t *T) {
	var i any = t
	if i == nil {
		sink(i)
	}
}

// --- dereferences ----------------------------------------------------------

func fieldSelection(p *T) int {
	if p == nil {
		return p.F
	}
	return 0
}

func load(p *int) int {
	if p == nil {
		return *p
	}
	return 0
}

func store(p *int) {
	if p == nil {
		*p = 1
	}
}

func mapUpdate(m map[string]int) {
	if m == nil {
		m["x"] = 1
	}
}

func rangeOverNilMap(m map[string]int) {
	if m == nil {
		for k := range m {
			sink(k)
		}
	}
}

func receiveFromNilChan(c chan int) {
	if c == nil {
		<-c
	}
}

func sendToNilChan(c chan int) {
	if c == nil {
		c <- 1
	}
}

func indexNilSlice(s []int) int {
	if s == nil {
		return s[0]
	}
	return 0
}

func rangeNilSlice(s []int) {
	if s == nil {
		for i, v := range s {
			sink(i, v)
		}
	}
}

func arrayIndex(p *[4]int) int {
	if p == nil {
		return p[0]
	}
	return 0
}

func arrayPtrRange(p *[4]int) {
	if p == nil {
		for i, v := range p {
			sink(i, v)
		}
	}
}

func sliceOperation(p *[4]int) []int {
	if p == nil {
		return p[:]
	}
	return nil
}

func typeAssertion(x any) int {
	if x == nil {
		return x.(int)
	}
	return 0
}

func dynamicMethodCall(i I) {
	if i == nil {
		i.M()
	}
}

func dynamicFunctionCall(f func()) {
	if f == nil {
		f()
	}
}

func deferAndGoNilFunc(f func()) {
	if f == nil {
		defer f()
		go f()
	}
}

// A *static* method call passes the receiver as an argument, not as the
// callee, so a nil receiver is not a nil dereference of the callee.
func staticMethodCallOnNilReceiver(p *T) {
	if p == nil {
		p.M()
		defer p.M()
	}
}

// --- panics ----------------------------------------------------------------

func panicNilError() {
	var e error
	panic(e)
}

// The zero value of a struct also has no constant value, but it is not
// nillable, so this is not a nil panic.
func panicZeroStruct() {
	var v struct{ A int }
	panic(v)
}

// --- facts learned from a comma-ok type assertion --------------------------

func commaOkPointer(x any) int {
	if p, ok := x.(*T); !ok {
		return p.F
	}
	return 0
}

// The asserted type is a bare interface, whose core type is nil, so
// `isNillable` is false and the else branch learns nothing.
func commaOkInterface(x any) {
	if v, ok := x.(I); !ok {
		v.M()
	}
}

// --- ChangeInterface fact expansion ----------------------------------------

type Wide interface {
	M()
	N()
}

func changeInterface(w Wide) {
	var i I = w
	if w == nil {
		i.M()
	}
}

// --- operations that are legal on a nil value ------------------------------

func legalOnNil(m map[string]int, s []int, c chan int) {
	if m == nil {
		sink(m["k"], len(m))
	}
	if s == nil {
		sink(len(s), s[:0], append(s, 1))
	}
	if c == nil {
		sink(len(c))
	}
}

// A second load of the same field is a different SSA value, so the fact
// learned about the first one does not carry.
type Named struct{ P *T }

func reloadedField(n *Named) int {
	if n.P == nil {
		return n.P.F
	}
	return 0
}

// --- generics --------------------------------------------------------------

// The constraint has no terms, so E may be instantiated as an interface and
// the boxed value's nilness is unknown.
func GenericAny[E any](e E) any {
	var i any = e
	if i == nil {
		return i
	}
	return nil
}

type PointerOnly interface{ ~*T }

// The constraint has terms, so the MakeInterface is statically non-nil.
func GenericConstrained[E PointerOnly](e E) any {
	var i any = e
	if i == nil {
		return i
	}
	return nil
}
