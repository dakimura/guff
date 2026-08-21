package forcetypeassert

// Upstream reports `n.Pos()` — the start of the assignment or the spec, never
// the `:=` token. The isolate key has no column, so only a golden can see that.

func blankAssign() {
	var a any
	_ = a.(int)
}

func defineFromIndex(m map[string]any) string {
	s := m["folder"].(string)

	return s
}

func plainAssignFromIndex(m map[string]any) string {
	var out string

	out = m["folder"].(string)

	return out
}

func valueSpec(a any) {
	var n = a.(int)
	_ = n
}

func inCondition(m map[string]any) bool {
	return m["to"].(string) != "idle"
}

func asArgument(a any) {
	takeInt(a.(int))
}

// `right hand must be only type assertion`: the assertion is buried in a call,
// so the two-value form could not have been written here.
func insideCall(a any) {
	n := takeIntRet(a.(int))
	_ = n
}

// Same message for the other reason — two values on the right.
func twoValues(a, b any) {
	x, y := a.(int), b.(int)
	_, _ = x, y
}

func takeInt(int)          {}
func takeIntRet(n int) int { return n }
