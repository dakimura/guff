package modernize

import "strings"

// Pattern 1 of stringscutprefix: which Trim call in the first statement of the
// `if` body is the one upstream pairs with the Has call. Each function's
// comment is what golangci-lint 2.12.2 does. See the Rust test for why.

func use(string)      {}
func suffix() string { return "/" }

func cutPlain(s string) { // FIRES
	if strings.HasSuffix(s, "/") {
		s = strings.TrimSuffix(s, "/")
	}
	use(s)
}

func cutNestedElse(s, t string, ok bool) { // FIRES: a mismatched Trim first, the match in the else (lima)
	if strings.HasSuffix(s, "/") {
		if ok {
			t = strings.TrimSuffix(t, "/")
		} else {
			s = strings.TrimSuffix(s, "/")
		}
	}
	use(s + t)
}

func cutOnlyMismatch(s, t string) { // silent: the only Trim has other arguments
	if strings.HasSuffix(s, "/") {
		t = strings.TrimSuffix(t, "/")
	}
	use(s + t)
}

func cutSecondStatement(s string) { // silent: the match is not in the first statement
	if strings.HasSuffix(s, "/") {
		use("x")
		s = strings.TrimSuffix(s, "/")
	}
	use(s)
}

func cutKindMismatchFirst(s string) { // FIRES: TrimSuffix is skipped, TrimPrefix matches
	if strings.HasPrefix(s, "/") {
		use(strings.TrimSuffix(s, "/") + strings.TrimPrefix(s, "/"))
	}
}

func cutCallArgument(s string) { // FIRES: arguments compared as syntax, a call equals itself
	if strings.HasSuffix(s, suffix()) {
		use(strings.TrimSuffix(s, suffix()))
	}
}

func cutParenthesized(s string) { // silent: `(s)` is not `s` syntactically
	if strings.HasSuffix(s, "/") {
		use(strings.TrimSuffix((s), "/"))
	}
}

func cutWithInit(s string) { // silent: an init statement
	if x := 1; strings.HasSuffix(s, "/") {
		use(strings.TrimSuffix(s, "/"))
		_ = x
	}
}

func cutTwoMatches(s string) { // FIRES once
	if strings.HasSuffix(s, "/") {
		use(strings.TrimSuffix(s, "/") + strings.TrimSuffix(s, "/"))
	}
}

func cutIndexExpr(m map[string]string) { // FIRES
	if strings.HasSuffix(m["k"], "/") {
		use(strings.TrimSuffix(m["k"], "/"))
	}
}
