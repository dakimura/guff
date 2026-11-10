// Not upstream: the walk scope and the literal shapes of unnecessary-if.
package shapes

var pkgLevel = func(c bool) bool {
	if c { // a closure in a package var: upstream walks FuncDecl bodies only
		return true
	} else {
		return false
	}
}

func nested(d bool) bool {
	// reported; upstream returns nil, so the if in the condition's closure is not visited
	if func() bool { if d { return true } else { return false } }() {
		return true
	} else {
		return false
	}
}

func notReported(c, d bool) bool {
	if c { // else has two statements; the inner if is visited and reported
		return true
	} else {
		_ = d
		if d {
			return true
		} else {
			return false
		}
	}
}

func parens(c, d bool, x int) (r bool) {
	if (c) {
		r = true
	} else {
		r = false
	}
	if c {
		return (true) // a parenthesized literal is not *ast.Ident
	} else {
		return false
	}
	if (x > 1) {
		r = false
	} else {
		r = true
	}
	if c {
		(r) = true
	} else {
		r = false
	}
	if c {
		r, d = true, false
	} else {
		r = false
	}
	go func() {
		if d {
			r = false
		} else {
			r = true
		}
	}()
	return r
}
