package directives

// `//exhaustive:` comment directives. Line numbers are asserted, so keep the
// shapes in this order and add new ones at the end.
//
// The same shapes live in compat/isolate/fixtures/exhaustive/directives.go,
// where golangci-lint is what produces the expected set; this copy is what
// lets the Rust side name which shape moved when the golden shifts.

type Color int

const (
	Red Color = iota
	Green
	Blue
)

// line 20: reported unless `explicit-exhaustive-switch`.
func switchPlain(c Color) string {
	switch c {
	case Red:
		return "r"
	}
	return ""
}

// line 30: silent unless `explicit-exhaustive-switch`.
func switchIgnore(c Color) string {
	//exhaustive:ignore
	switch c {
	case Red:
		return "r"
	}
	return ""
}

// line 40: reported either way.
func switchEnforce(c Color) string {
	//exhaustive:enforce
	switch c {
	case Red:
		return "r"
	}
	return ""
}

// line 52: `enforce-default-case-required` is `defrequire=1`, not an
// `//exhaustive:enforce`; it demands the default case, and since v0.15.0 the
// missing members are reported alongside it.
func switchEnforceDefaultCase(c Color) string {
	//exhaustive:enforce-default-case-required
	switch c {
	case Red:
		return "r"
	}
	return ""
}

// line 63: every member listed, no default — only `default-case-required` has
// anything to say, and the directive waives it.
func switchIgnoreDefaultCase(c Color) string {
	//exhaustive:ignore-default-case-required
	switch c {
	case Red:
		return "r"
	case Green:
		return "g"
	case Blue:
		return "b"
	}
	return ""
}

// line 76: the same switch without the directive.
func switchAllMembers(c Color) string {
	switch c {
	case Red:
		return "r"
	case Green:
		return "g"
	case Blue:
		return "b"
	}
	return ""
}

// line 90: an unknown name is an error, and the switch is not checked.
func switchIgnorePrefix(c Color) string {
	//exhaustive:ignoreme
	switch c {
	case Red:
		return "r"
	}
	return ""
}

// line 100: a trailing comment is associated with no node, so it enforces
// nothing.
func switchTrailing(c Color) string {
	switch c { //exhaustive:enforce
	case Red:
		return "r"
	}
	return ""
}

// line 108
var mapPlain = map[Color]string{Red: "r"}

// line 113
//
//exhaustive:ignore
var mapIgnore = map[Color]string{Red: "r"}

// line 118
//
//exhaustive:enforce
var mapEnforce = map[Color]string{Red: "r"}

// The default-case directives (the deprecated names of `defrequire=0/1`) are
// neither ignore nor enforce, and a map literal has no default case.
//
// line 126
//
//exhaustive:ignore-default-case-required
var mapIgnoreDefaultCase = map[Color]string{Red: "r"}

// line 131
//
//exhaustive:enforce-default-case-required
var mapEnforceDefaultCase = map[Color]string{Red: "r"}

// line 136
func mapReturn() map[Color]string {
	//exhaustive:enforce
	return map[Color]string{Red: "r"}
}

// line 143: a directive on the enclosing FuncDecl reaches nothing.
//
//exhaustive:enforce
func mapFuncDoc() map[Color]string {
	return map[Color]string{Red: "r"}
}

// line 152: `compositeLitComments` walks out through any node to the first
// `AssignStmt` or `GenDecl`, so this reaches the literal inside the func
// literal.
//
//exhaustive:enforce
var mapThroughFuncLit = func() map[Color]string {
	return map[Color]string{Red: "r"}
}()

// line 159
//
//exhaustive:ignore
var mapIgnoreThroughFuncLit = func() map[Color]string {
	return map[Color]string{Red: "r"}
}()

// Elided composite-literal types: checked since exhaustive v0.15.0, which
// asks for the literal's own recorded type (v0.13.0 asked its type expression).
//
// line 166
var elidedNested = map[Color]map[Color]string{
	Red:   {Red: "r"},
	Green: {Red: "r"},
	Blue:  {Red: "r"},
}

// line 173: the control — spelt out, so it is checked.
var elidedSpeltOut = map[Color]map[Color]string{
	Red:   map[Color]string{Red: "r"},
	Green: map[Color]string{Red: "r"},
	Blue:  map[Color]string{Red: "r"},
}

type colorHolder struct {
	m map[Color]string
}

// line 184
var elidedInStruct = []colorHolder{
	{m: map[Color]string{Red: "r"}},
}

type ColorNames map[Color]string

// line 191
var elidedNamed = map[Color]ColorNames{
	Red:   {Red: "r"},
	Green: {Red: "r"},
	Blue:  {Red: "r"},
}
