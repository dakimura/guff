package slicescontains

func containsReturn(s []int, needle int) bool {
	for _, v := range s {
		if v == needle {
			return true
		}
	}
	return false
}

func containsIndexElem(s []int, needle int) bool {
	for i := range s {
		if s[i] == needle {
			return true
		}
	}
	return false
}

func containsFuncPred(s []int) bool {
	pred := func(v int) bool { return v > 0 }
	for _, v := range s {
		if pred(v) {
			return true
		}
	}
	return false
}

func containsBreakBody(s []int, needle int) {
	for _, v := range s {
		if v == needle {
			println("found")
			break
		}
	}
}

func containsFoundAssign(s []int, needle int) bool {
	found := false
	for _, v := range s {
		if v == needle {
			found = true
			break
		}
	}
	return found
}

func skipSoleBreak(s []int, needle int) {
	for _, v := range s {
		if v == needle {
			break
		}
	}
}

func alreadyContains(s []int, needle int) bool {
	return contains(s, needle)
}

func contains(s []int, needle int) bool {
	for _, v := range s {
		_ = v
		_ = needle
	}
	return false
}

// The `ContainsFunc` half reads the predicate's signature and declines twice:
// a variadic predicate, and one whose parameter type is not *identical* to the
// slice's element type. Assignability is not enough — `slices.ContainsFunc`
// instantiates `F ~func(E) bool` with the element type, so a predicate taking
// an interface the element merely implements would not compile.
//
// k6 `internal/js/modules/k6/grpc/client.go:445` is the second shape: `stack`
// is `[]*sobek.Object` while `SameAs(other Value) bool` takes the interface.

type value interface{ isValue() }

type obj struct{ id int }

func (o *obj) isValue() {}

func (o *obj) sameAs(other value) bool { return other != nil }

func ifacePred(v value) bool { return v != nil }

func objPred(o *obj) bool { return o != nil }

func variadicPred(o *obj, rest ...int) bool { return o != nil }

// Skipped: the parameter is an interface the element implements.
func skipMethodValueOnIface(o *obj, stack []*obj) bool {
	for _, vis := range stack {
		if o.sameAs(vis) {
			return true
		}
	}
	return false
}

func skipFreeFuncOnIface(stack []*obj) bool {
	for _, vis := range stack {
		if ifacePred(vis) {
			return true
		}
	}
	return false
}

// Skipped: variadic.
func skipVariadic(stack []*obj) bool {
	for _, vis := range stack {
		if variadicPred(vis) {
			return true
		}
	}
	return false
}

// Reported: the parameter type is the element type.
func containsFuncIdenticalParam(stack []*obj) bool {
	for _, vis := range stack {
		if objPred(vis) {
			return true
		}
	}
	return false
}

// Reported: the element type *is* the interface.
func containsFuncIfaceElem(stack []value) bool {
	for _, vis := range stack {
		if ifacePred(vis) {
			return true
		}
	}
	return false
}
