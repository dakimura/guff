// Package fields is unused's struct-field half.
//
// honnef models a named struct type as *owning* its fields (`edgeKindOwn`).
// They are candidates in their own right, but a field is reported only when its
// owner type is used: otherwise the type itself is the finding and
// `colorAndQuieten` silences everything it owns.
//
// The rules exercised here, by their numbers in `unused/unused.go`:
//
//	(5.1)  converting between equivalent structs makes their fields use each
//	       other — not used outright
//	(6.1)  fields of type NoCopy sentinel
//	(6.2)  exported fields
//	(6.3)  embedded fields that help implement an interface
//	(6.4)  embedded fields that have exported methods
//	(6.5)  embedded structs that have exported fields
//	(7.1)  field accesses use fields, including every embedded field on the
//	       path of a promoted access
//	(7.2)  fields use their types — an edge *from the field*, which is why
//	       `type outer struct { inner }` reports `inner` and `inner`'s own type
//	(11.1) anonymous struct types use all their fields
//
// `FieldWritesAreUses` and `ExportedFieldsAreUsed` are both on by default, so a
// write is a use and an exported field is never reported.
//
// (5.2) `unsafe.Pointer` and (6.6) `structs.HostLayout` need imports, which the
// unit-test type-checker has no importer for; they live in `fieldsunsafe/`,
// which only the golden case materialises.
package fields

// A used struct: one field read, one never, plus the exported forms.
type used struct {
	read   int
	unread int
	Exp    int
	ExpTag int `json:"exp_tag"`
	unrTag int `json:"unr_tag"` // a tag does not save it
}

func Use() int {
	u := used{}
	return u.read
}

// The whole type is unused, so the type is the finding and `a`/`b` go quiet.
type neverUsed struct {
	a int
	b int
}

// An unkeyed composite literal names no field, so upstream walks the struct and
// uses them all.
type positional struct {
	x int
	y int
}

func UsePositional() positional { return positional{1, 2} }

// A keyed one uses only the keys it names.
type keyed struct {
	set   int
	unset int
}

func UseKeyed() keyed { return keyed{set: 1} }

// A write is a use; a read through a pointer is too.
type viaPtr struct {
	r int
	w int
}

func UseViaPtr(p *viaPtr) int {
	p.w = 1
	return p.r
}

// (6.1) a `noCopy` sentinel: an empty struct with Lock and Unlock. It exists to
// be found by `go vet`, never to be read.
type noCopy struct{}

func (noCopy) Lock()   {}
func (noCopy) Unlock() {}

type withNoCopy struct {
	_    noCopy
	nc   noCopy
	dead int
}

func UseWithNoCopy() withNoCopy { return withNoCopy{} }

// (6.3) an embedded field whose type contributes a method an interface in this
// package requires — even an unexported one.
type quiet interface{ speak() }

type quietImpl struct{}

func (quietImpl) speak() {}

type embedsQuiet struct {
	quietImpl
	deadQ int
}

func UseEmbedsQuiet(e embedsQuiet) quiet { return e }

// (6.4) an embedded field whose type has an exported method, with no interface
// in sight.
type hasExportedMethod struct{}

func (hasExportedMethod) Exported() {}

type embedsExported struct {
	hasExportedMethod
	deadE int
}

func UseEmbedsExported() embedsExported { return embedsExported{} }

// (6.5) an embedded struct that has an exported field.
type hasExportedField struct{ Exp int }

type embedsExportedField struct {
	hasExportedField
	deadF int
}

func UseEmbedsExportedField() embedsExportedField { return embedsExportedField{} }

// None of the three: the embedded field is reported, and because (7.2) runs
// *from* the field, its type is reported too.
type plainInner struct{ q int }

type embedsPlain struct {
	plainInner
	deadP int
}

func UseEmbedsPlain() embedsPlain { return embedsPlain{} }

// (7.1) reading a promoted field reads every embedded field on the path.
type promotedInner struct {
	p        int
	deadProm int
}

type promotedOuter struct {
	promotedInner
	own int
}

func UsePromoted(o promotedOuter) int { return o.p + o.own }

// (11.1) an anonymous struct type uses all of its fields.
func UseAnon() int {
	v := struct {
		hidden int
		other  int
	}{}
	return v.hidden
}

// (5.1) equivalent structs converted into each other. One pair is accessed
// afterwards and lives; the other is not, and both sides are reported.
type convA struct{ n int }
type convB struct{ n int }

func UseConvPair(a convA) int {
	b := convB(a)
	return b.n
}

type cvA struct{ deadCv int }
type cvB struct{ deadCv int }

func UseCv(a cvA) cvB { return cvB(a) }

// An exported type is still asked about its unexported fields.
type Exported struct {
	Public  int
	private int
}

func UseExported() Exported { return Exported{} }

// A blank field cannot be referred to.
type blanks struct {
	_        int
	deadBlnk int
}

func UseBlanks() blanks { return blanks{} }

// An embedded interface contributes its methods.
type Talker interface{ Talk() }

type embedsIface struct {
	Talker
	deadI int
}

func UseEmbedsIface() embedsIface { return embedsIface{} }

// A field read only inside a function nothing calls is not read at all.
type onlyInDead struct{ f int }

func deadReader(o onlyInDead) int { return o.f }

func UseOnlyInDead() onlyInDead { return onlyInDead{} }

// Fields are objects, not names: the same name on two types is two candidates.
type sameNameA struct{ shared int }
type sameNameB struct{ shared int }

func UseSameName(a sameNameA) int { return a.shared }

func UseSameNameB() sameNameB { return sameNameB{} }

// A type declared inside a function is a named type like any other.
func UseLocalType() int {
	type localStruct struct {
		live    int
		deadLoc int
	}
	v := localStruct{}
	return v.live
}

// A generic struct: `b.v` on a `box[int]` denotes a *substituted* field object,
// so the lookup has to come back to the origin.
type box[T any] struct {
	v       T
	deadBox int
}

func UseBox(b box[int]) int { return b.v }

// A struct that is never built, only named as a map value type.
type mapValue struct{ m int }

func UseMapValue() map[string]mapValue { return nil }

// A struct with a field of its own type.
type node struct {
	next     *node
	deadNode int
}

func UseNode(n *node) *node { return n.next }

// honnef reads the embedded fields on a *method* selection's path too, so
// calling a promoted method — even an unexported one, with no interface and no
// exported method anywhere — keeps the embedded field alive.
type mumbler struct{ deadIn int }

func (mumbler) mumble() {}

type outerCall struct {
	mumbler
	deadOut int
}

func UseOuterCall(o outerCall) { o.mumble() }

// The same shape with the method never called: now the embedded field, its
// type and the method are all findings.
type mumbler2 struct{ deadIn2 int }

func (mumbler2) mumble2() {}

type outerNoCall struct {
	mumbler2
	deadOut2 int
}

func UseOuterNoCall() outerNoCall { return outerNoCall{} }

// A method *expression* walks the same path.
type mumbler3 struct{ deadIn3 int }

func (mumbler3) mumble3() {}

type outerExpr struct {
	mumbler3
	deadOut3 int
}

func UseOuterExpr() func(outerExpr) { return outerExpr.mumble3 }

// `//lint:ignore U1000` on a *type* covers its fields too — upstream's "use
// methods and fields of ignored types" — even though the directive's own line
// is nowhere near them.
//
//lint:ignore U1000 kept on purpose
type ignoredType struct {
	live       int
	deadIgnord int
}

func UseIgnored() int {
	v := ignoredType{}
	return v.live
}
