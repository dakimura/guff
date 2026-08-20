package unparam

type kind2 int

var errNotFound2 = newErr2()

func newErr2() error { return nil }

func allUsed(x int, y string) {
	_ = x
	println(y)
}

func explicitKeep(unused int) {
	_ = unused
}

func emptyBody(unused int) {}

func onlyReturn(unused int) {
	return
}

// Used as a value (callback); signature must stay — unused param is OK.
func asCallback(prefix string) {
	println("ok")
}

var callbacks = []func(string){asCallback}

func takesHandler(h func(x int)) { h(1) }

func callWithLit() {
	takesHandler(func(unused int) {
		println("handler")
	})
}

// `dummyImpl`: a body that only returns constants is skipped entirely, which is
// why this is not "result 1 is always nil".
func dummyReturn() (int, error) {
	return 0, nil
}

// Same, through a call the harmless-call pattern allows (`\berrors\b`).
func dummyErrors() (int, error) {
	return 1, errNotFound2
}

func useDummies() {
	_, _ = dummyReturn()
	_, _ = dummyErrors()
	_, _ = dummyReturn()
	_, _ = dummyErrors()
}

// `return f(...)` fixes f's results: the call is part of the return, so the
// second result cannot be dropped.
func fixedResults(name string) (string, kind2, error) {
	if name == "" {
		return "", 0, errNotFound2
	}
	if name == "x" {
		return name, 0, nil
	}
	return "", 0, errNotFound2
}

func forwards(name string) (string, kind2, error) {
	return fixedResults(name)
}

// Exported: the call sites may be anywhere, so no constant-parameter report.
func Respond(status int, msg string) string {
	if status > 0 {
		return msg
	}
	return ""
}

func useRespond() string {
	return Respond(200, "a") + Respond(200, "b") + Respond(200, "c") + Respond(200, "d")
}

// Only three call sites: below upstream's threshold of four.
func threeSites(status int) int { return status }

func useThreeSites() int { return threeSites(7) + threeSites(7) + threeSites(7) }
