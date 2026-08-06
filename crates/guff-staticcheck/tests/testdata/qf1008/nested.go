package pkg

// Upstream resolves the enclosing path of a selector all the way to the file,
// so a selector nested anywhere inside another selector's operand is skipped —
// even across a func literal, as in prometheus
// `discovery/kubernetes/endpointslice_test.go:364`.

type NestedOuter struct{ NestedInner }
type NestedInner struct{ F1 int }

type harness struct {
	afterStart func()
}

func (h harness) Run() {}

func fnNestedInCompositeLitCall() {
	var o NestedOuter
	harness{
		afterStart: func() {
			// Enclosed by `harness{…}.Run` — not flagged.
			_ = o.NestedInner.F1
		},
	}.Run()
}

func fnNestedInCallArg() {
	var o NestedOuter
	// The argument's `o.NestedInner.F1` is enclosed by the outer selector and so
	// not flagged; the outer `sink(…).NestedInner.F1` segment is.
	_ = sink(o.NestedInner.F1).NestedInner.F1
}

type sinkResult = NestedOuter

func sink(int) sinkResult { return NestedOuter{} }

func fnNotNested() {
	var o NestedOuter
	h := harness{
		afterStart: func() {
			// No enclosing selector — flagged.
			_ = o.NestedInner.F1
		},
	}
	_ = h
}
