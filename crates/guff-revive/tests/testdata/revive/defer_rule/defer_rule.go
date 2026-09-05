// Package deferrule holds one function per `defer` sub-check.
//
// The rule has six of them — loop, callChain, methodCall, return, recover and
// immediateRecover — and its **argument list picks which run**:
// `arguments: [["immediate-recover", "recover", "return"]]` turns the other
// three off, while no arguments at all means every one. The two
// `revive-defer-*` golden cases read this file under both configurations, so a
// sub-check that stops respecting the arguments shows up in one of them.
package deferrule

import "os"

type A struct{}
type B struct{}
type V struct{}

func (a *A) B() *B           { return &B{} }
func (a *A) Cleanup() func() { return func() {} }
func (b *B) C()              {}
func (v V) M()               {}

// loop: defer inside a for.
func InLoop(names []string) {
	for _, n := range names {
		f, err := os.Open(n)
		if err != nil {
			continue
		}
		defer f.Close()
	}
}

// loop: defer inside a range over a map.
func InRange(m map[string]*os.File) {
	for _, f := range m {
		defer f.Close()
	}
}

// loop, but inside a func literal: the literal resets the loop state.
func LoopThenFuncLit(names []string) {
	for range names {
		func() {
			defer println("not in a loop any more")
		}()
	}
}

// silent under every sub-check: `a.B().C` is a selector whose X happens to be a
// call, and upstream matches on the *callee* being a call — so this is neither
// a callChain nor a methodCall.
func NotACallChain(a *A) {
	defer a.B().C()
}

// callChain: the deferred callee is itself a call, so the defer runs whatever
// it returns. tailscale writes `defer b.CheckDeadlocks()()` thirteen times.
func CallChain(a *A) {
	defer a.Cleanup()()
}

// methodCall: the receiver is a *type*, so this is a method expression.
func MethodCall(v V) {
	defer V.M(v)
}

// immediateRecover: recover() is the deferred call.
func ImmediateRecover() {
	defer recover()
}

// immediateRecover: recover() evaluated as an argument of the deferred call.
func RecoverAsArgument() {
	defer sink(recover())
}

func sink(any) {}

// return: a deferred literal that returns a value.
func ReturnInDefer() {
	defer func() error {
		return nil
	}()
}

// return, but two literals deep: only the top-level one counts.
func ReturnInNestedDefer() {
	defer func() {
		_ = func() error {
			return nil
		}
	}()
}

// recover: outside any deferred function, in an assignment.
func RecoverInAssignment() {
	_ = recover()
}

// recover: inside an if, which a walk over statement bodies alone would miss.
func RecoverInIf(b bool) {
	if b {
		_ = recover()
	}
}

// recover: as a call argument. Upstream stops at the outer call and never
// looks at its arguments, so this is silent.
func RecoverAsCallArgument() {
	sink(recover())
}

// silent: recover() inside a deferred func literal is the correct use.
func ProperRecover() {
	defer func() {
		_ = recover()
	}()
}
