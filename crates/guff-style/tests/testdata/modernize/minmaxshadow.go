// The rewrite writes `min(a, b)`, so `min` has to still *mean* the builtin
// where it lands:
//
//	if !is[*types.Builtin](lookup(pass.TypesInfo, curIfStmt, sym)) {
//	    return // min/max function is shadowed
//	}
//
// guff had no such check. beats' `x-pack/filebeat/input/awss3` declares its own
// package-level `min` — which modernize reports separately as a user-defined
// one equivalent to the builtin — and while it is there every `min` rewrite in
// that package is silent upstream, `max` still fires, and guff reported
// `s3_objects_test.go:517` alone.
//
// A local shadow counts too (upstream's own testdata has `hour, min := 3600,
// 60`), which is why the lookup starts at the innermost scope.
package minmax

// This declaration is itself a finding — the other arm of the rule — and it is
// what shadows `min` for everything below.
func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}

// silent: `min` is this package's function, not the builtin.
func shadowedIf(a, b int) int {
	x := a
	if x > b {
		x = b
	}
	return x
}

// silent: the if/else arm, same reason.
func shadowedIfElse(a, b int) int {
	var x int
	if a > b {
		x = b
	} else {
		x = a
	}
	return x
}

// FINDING: nothing shadows `max`.
func maxIf(a, b int) int {
	x := a
	if x < b {
		x = b
	}
	return x
}

// FINDING: the if/else arm of `max`.
func maxIfElse(a, b int) int {
	var x int
	if a < b {
		x = b
	} else {
		x = a
	}
	return x
}

// silent: a *local* `max` shadows the builtin here, even though the package
// does not.
func shadowedLocally(hour int) int {
	max := 60
	var t int
	if hour < max {
		t = max
	} else {
		t = hour
	}
	return t
}
