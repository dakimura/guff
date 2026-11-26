package bodyclose

import "net/http"

// A response stored into a **struct field** is settled by *any* closed body in
// the same **basic block**, whichever response that body belongs to.
//
// Upstream reaches this through `*ssa.Store` -> `*ssa.FieldAddr` and then
// scans the whole block:
//
//	if s, ok := aref.(*ssa.Store); ok {
//	    if f, ok := s.Addr.(*ssa.FieldAddr); ok {
//	        for _, bRef := range f.Block().Instrs {   // the WHOLE basic block
//	            bOp, ok := r.getBodyOp(bRef)
//	            ...
//	            for _, ccall := range *bOp.Referrers() {
//	                if r.isCloseCall(ccall) { return false }
//	            }
//	        }
//	    }
//	}
//
// No value tracking at all — which is why teleport's `errors_test.go` closes
// one body and carries the comment "The bodyclose linter trips if we don't
// explicitly close the body, even though it's a no-op in this case."
//
// Every shape below was measured against golangci-lint 2.12.2. The silent and
// the reported halves are both here on purpose: a gate that suppressed every
// field store, or one that reported every one, would be green against half of
// them.

type fbHolder struct {
	Resp  *http.Response
	Other *http.Response
}

type fbOuter struct {
	In fbHolder
}

// Any function returning `*http.Response` is skipped whole by upstream's
// `FuncLoop`, so this helper is a source of responses and never a finding.
func fbBuild() *http.Response { return &http.Response{} }

func fbSink() {}

// ---- the store is settled by a close in its own block ----------------------

// FbOneClosed: the ordinary shape.
func FbOneClosed() {
	h := &fbHolder{Resp: fbBuild()}
	h.Resp.Body.Close()
}

// FbTwoOnlyFirstClosed: one close, two stores — the block scan settles both.
func FbTwoOnlyFirstClosed() {
	h1 := &fbHolder{Resp: fbBuild()}
	h2 := &fbHolder{Resp: fbBuild()}
	h1.Resp.Body.Close()
	_ = h2
}

// FbValueLiteralClosed: a non-pointer literal stores through a `FieldAddr` too.
func FbValueLiteralClosed() {
	h := fbHolder{Resp: fbBuild()}
	h.Resp.Body.Close()
}

// FbFieldAssignClosed: an assignment to an existing field, not a literal.
func FbFieldAssignClosed() {
	var h fbHolder
	h.Resp = fbBuild()
	h.Resp.Body.Close()
}

// FbSilencedByLocalClose: the close is on an unrelated *local* response. The
// scan asks only whether some body was closed in the block.
func FbSilencedByLocalClose() {
	h := &fbHolder{Resp: fbBuild()}
	r := fbBuild()
	r.Body.Close()
	_ = h
}

// FbNestedFieldClosed: a field of a field.
func FbNestedFieldClosed() {
	o := &fbOuter{In: fbHolder{Resp: fbBuild()}}
	o.In.Resp.Body.Close()
}

// FbTwoFieldsOneClosed: two fields of one struct, one close.
func FbTwoFieldsOneClosed() {
	h := &fbHolder{Resp: fbBuild(), Other: fbBuild()}
	h.Resp.Body.Close()
}

// FbClosedBefore: the block is scanned from its start, so a close written
// *above* the store settles it just the same.
func FbClosedBefore() {
	r := fbBuild()
	r.Body.Close()
	h := &fbHolder{Resp: fbBuild()}
	_ = h
}

// FbClosedThroughOtherField: closed through a different field of the same
// struct — nil at run time, and still a close as far as the scan is concerned.
func FbClosedThroughOtherField() {
	h := &fbHolder{Resp: fbBuild()}
	h.Other = fbBuild()
	h.Other.Body.Close()
}

// FbInLoopClosed: a loop body is a block like any other.
func FbInLoopClosed(n int) {
	for i := 0; i < n; i++ {
		h := &fbHolder{Resp: fbBuild()}
		h.Resp.Body.Close()
	}
}

// FbInFuncLitClosed: a func literal is its own function, with its own blocks.
func FbInFuncLitClosed() {
	f := func() {
		h := &fbHolder{Resp: fbBuild()}
		h.Resp.Body.Close()
	}
	f()
}

// FbCloseBeforeBranch: the branch comes after both, so both are in block 0.
func FbCloseBeforeBranch(cond bool) {
	h := &fbHolder{Resp: fbBuild()}
	h.Resp.Body.Close()
	if cond {
		fbSink()
	}
}

// FbCloseInBareBlock: a bare `{ … }` is a lexical scope and not a branch, so
// go/ssa keeps both in one block.
func FbCloseInBareBlock() {
	h := &fbHolder{Resp: fbBuild()}
	{
		h.Resp.Body.Close()
	}
}

// FbStoreAndCloseInCase: both inside one `case` body.
func FbStoreAndCloseInCase(n int) {
	switch n {
	case 1:
		h := &fbHolder{Resp: fbBuild()}
		h.Resp.Body.Close()
	}
}

// FbCloseAfterDefer: a `defer` does not end a block.
func FbCloseAfterDefer() {
	h := &fbHolder{Resp: fbBuild()}
	defer fbSink()
	h.Resp.Body.Close()
}

// FbDeferredFieldClose: the close itself deferred.
func FbDeferredFieldClose() {
	h := &fbHolder{Resp: fbBuild()}
	defer h.Resp.Body.Close()
}

// FbCloseAfterCall: a plain call does not end a block either.
func FbCloseAfterCall() {
	h := &fbHolder{Resp: fbBuild()}
	fbSink()
	h.Resp.Body.Close()
}

// ---- the store is reported --------------------------------------------------

// FbOneNotClosed: nothing in the block closes anything.
func FbOneNotClosed() {
	h := &fbHolder{Resp: fbBuild()}
	_ = h
}

// FbTwoNoneClosed: two of them.
func FbTwoNoneClosed() {
	h1 := &fbHolder{Resp: fbBuild()}
	h2 := &fbHolder{Resp: fbBuild()}
	_, _ = h1, h2
}

// FbClosedInBranch: the close sits in the `if` body, which is its own block —
// so even the response that *is* closed is reported.
func FbClosedInBranch(cond bool) {
	h1 := &fbHolder{Resp: fbBuild()}
	h2 := &fbHolder{Resp: fbBuild()}
	if cond {
		h1.Resp.Body.Close()
	}
	_ = h2
}

// FbSliceElemClosed: a slice element is an `IndexAddr`, not a `FieldAddr`, so
// the block scan never runs and the response stays reportable.
func FbSliceElemClosed() {
	s := []*http.Response{fbBuild()}
	s[0].Body.Close()
}

// FbMapValueClosed: a map value is a `MapUpdate`. Same conclusion.
func FbMapValueClosed() {
	m := map[string]*http.Response{"a": fbBuild()}
	m["a"].Body.Close()
}

// FbInLoopNotClosed: the loop body closes nothing.
func FbInLoopNotClosed(n int) {
	for i := 0; i < n; i++ {
		h := &fbHolder{Resp: fbBuild()}
		_ = h
	}
}

// FbStoreInline: never bound to a variable, never closed.
func FbStoreInline() {
	fbUse(&fbHolder{Resp: fbBuild()})
}

func fbUse(h *fbHolder) { _ = h }

// FbCloseAfterBranch: the store's block ends at the `if`, so the close is in a
// later block and settles nothing.
func FbCloseAfterBranch(cond bool) {
	h := &fbHolder{Resp: fbBuild()}
	if cond {
		fbSink()
	}
	h.Resp.Body.Close()
}

// FbCloseAfterLoop: a loop between the two, likewise.
func FbCloseAfterLoop(n int) {
	h := &fbHolder{Resp: fbBuild()}
	for i := 0; i < n; i++ {
		fbSink()
	}
	h.Resp.Body.Close()
}

// FbCloseAfterAndAnd: a short-circuit `&&` compiles to a branch and a merge
// block, so it separates the store from the close as surely as an `if` does.
func FbCloseAfterAndAnd(a, b bool) {
	h := &fbHolder{Resp: fbBuild()}
	_ = a && b
	h.Resp.Body.Close()
}

// FbCloseInSwitchCase: the store before the `switch`, the close in a `case`.
func FbCloseInSwitchCase(n int) {
	h := &fbHolder{Resp: fbBuild()}
	switch n {
	case 1:
		h.Resp.Body.Close()
	}
}

// FbFieldAssignNotClosed: guff used to *drop* this shape — an assignment to a
// field selector bound no name, so nothing ever reported it. A miss, not an
// over-report, and the only one in the group.
func FbFieldAssignNotClosed() {
	var h fbHolder
	h.Resp = fbBuild()
	_ = h
}

// FbTwoLocalsOneClosed: the "any close in the block" rule belongs to the
// `FieldAddr` store alone. Two plain locals stay value-tracked, so the second
// one is still a finding.
func FbTwoLocalsOneClosed() {
	r1 := fbBuild()
	r2 := fbBuild()
	r1.Body.Close()
	_ = r2
}
