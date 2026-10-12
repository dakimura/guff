// Package ind drives printf's interface-method induction through every place
// refactor/satisfy records an assignability constraint.
package ind

import "fmt"

type impl int

func (impl) Logf(format string, args ...any) { print(fmt.Sprintf(format, args...)) }

type ptrImpl int

func (*ptrImpl) Logf(format string, args ...any) { print(fmt.Sprintf(format, args...)) }

type printImpl int

func (printImpl) Log(args ...any) { print(fmt.Sprint(args...)) }

// Each interface below is related to an implementation in one way only.

type IArg interface{ Logf(format string, args ...any) }

func takeArg(IArg) {}
func _()         { takeArg(impl(0)) }
func _(l IArg)   { l.Logf("%s", 1) }

type IVariadic interface{ Logf(format string, args ...any) }

func takeVariadic(...IVariadic) {}
func _()                       { takeVariadic(impl(0), impl(1)) }
func _(l IVariadic)            { l.Logf("%s", 2) }

type IReturn interface{ Logf(format string, args ...any) }

func ret() IReturn        { return impl(0) }
func _(l IReturn)         { l.Logf("%s", 3) }

type IReturnN interface{ Logf(format string, args ...any) }

func two() (impl, error)  { return 0, nil }
func retN() (IReturnN, error) { return two() }
func _(l IReturnN)        { l.Logf("%s", 4) }

type IStruct interface{ Logf(format string, args ...any) }
type holder struct{ l IStruct }

var _ = holder{l: impl(0)}

func _(l IStruct) { l.Logf("%s", 5) }

type IStructPos interface{ Logf(format string, args ...any) }
type holderPos struct{ l IStructPos }

var _ = &holderPos{impl(0)}

func _(l IStructPos) { l.Logf("%s", 6) }

type IMapVal interface{ Logf(format string, args ...any) }

var _ = map[string]IMapVal{"a": impl(0)}

func _(l IMapVal) { l.Logf("%s", 7) }

type IMapKey interface{ Logf(format string, args ...any) }

var mk map[IMapKey]int

func _() { _ = mk[impl(0)] }
func _(l IMapKey) { l.Logf("%s", 8) }

type ISlice interface{ Logf(format string, args ...any) }

var _ = []ISlice{impl(0)}

func _(l ISlice) { l.Logf("%s", 9) }

type ICompare interface{ Logf(format string, args ...any) }

func _(l ICompare) bool { return l == impl(0) }
func _(l ICompare)      { l.Logf("%s", 10) }

type IConvert interface{ Logf(format string, args ...any) }

var _ = IConvert(impl(0))

func _(l IConvert) { l.Logf("%s", 11) }

type ISend interface{ Logf(format string, args ...any) }

func _(c chan ISend) { c <- impl(0) }
func _(l ISend)      { l.Logf("%s", 12) }

type IAssert interface{ Logf(format string, args ...any) }

func _(x IAssert) { _ = x.(impl) }
func _(l IAssert) { l.Logf("%s", 13) }

type ITypeSwitch interface{ Logf(format string, args ...any) }

func _(x ITypeSwitch) {
	switch x.(type) {
	case impl, nil:
	}
}
func _(l ITypeSwitch) { l.Logf("%s", 14) }

type ISwitch interface{ Logf(format string, args ...any) }

func _(x ISwitch) {
	switch x {
	case impl(0):
	}
}
func _(l ISwitch) { l.Logf("%s", 15) }

type IRange interface{ Logf(format string, args ...any) }

func _(xs []impl) {
	var v IRange
	for _, v = range xs {
	}
	_ = v
}
func _(l IRange) { l.Logf("%s", 16) }

type IAppend interface{ Logf(format string, args ...any) }

func _(xs []IAppend) { _ = append(xs, impl(0)) }
func _(l IAppend)    { l.Logf("%s", 17) }

type IAssign interface{ Logf(format string, args ...any) }

func _() {
	var l IAssign
	l = impl(0)
	_ = l
}
func _(l IAssign) { l.Logf("%s", 18) }

type IPtr interface{ Logf(format string, args ...any) }

var _ IPtr = new(ptrImpl)

func _(l IPtr) { l.Logf("%s", 19) }

type embeds struct{ impl }
type IEmbed interface{ Logf(format string, args ...any) }

var _ IEmbed = embeds{}

func _(l IEmbed) { l.Logf("%s", 20) }

type IPrint interface{ Log(args ...any) }

var _ IPrint = printImpl(0)

func _(l IPrint) { l.Log("%s", 21) }

// Interface to interface: IOuter is implemented by IArg, which is a wrapper.
type IOuter interface{ Logf(format string, args ...any) }

func _(l IArg) IOuter { return l }
func _(l IOuter)      { l.Logf("%s", 22) }

// A generic implementation.
type gen[T any] struct{}

func (gen[T]) Logf(format string, args ...any) { print(fmt.Sprintf(format, args...)) }

type IGen interface{ Logf(format string, args ...any) }

var _ IGen = gen[int]{}

func _(l IGen) { l.Logf("%s", 23) }

// Not printf: the implementation does not forward.
type quiet int

func (quiet) Logf(format string, args ...any) {}

type IQuiet interface{ Logf(format string, args ...any) }

var _ IQuiet = quiet(0)

func _(l IQuiet) { l.Logf("%s", 24) }

// A local closure's return and a func literal argument.
type IClosure interface{ Logf(format string, args ...any) }

var _ = func() IClosure { return impl(0) }

func _(l IClosure) { l.Logf("%s", 25) }

// Embedded interface method of this package.
type IBase interface{ Logf(format string, args ...any) }
type IDerived interface {
	IBase
	Other()
}
type both struct{ impl }

func (both) Other() {}

var _ IDerived = both{}

func _(l IBase)    { l.Logf("%s", 26) }
func _(l IDerived) { l.Logf("%s", 27) }

// Multi-value assign from a map lookup.
type IComma interface{ Logf(format string, args ...any) }

func _(m map[string]impl) {
	var l IComma
	var ok bool
	l, ok = m["x"]
	_, _ = l, ok
}
func _(l IComma) { l.Logf("%s", 28) }

// Generic function argument.
type IGenArg interface{ Logf(format string, args ...any) }

func takeGen[T IGenArg](T) {}
func _()                  { takeGen[impl](impl(0)) }
func _(l IGenArg)         { l.Logf("%s", 29) }

// go/defer call arguments.
type IGo interface{ Logf(format string, args ...any) }

func takeGo(IGo) {}
func _()        { go takeGo(impl(0)); defer takeGo(impl(0)) }
func _(l IGo)   { l.Logf("%s", 30) }
