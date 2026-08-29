package unparam

type secKind int

const statusOK = 200

var errNotFound = newErr()

func newErr() error { return nil }

func example(used int, unused string) int {
	return used + 1
}

func withBlank(_ int, y int) int {
	return y
}

func stub(unused int) {
	panic("not implemented")
}

func discardOnly(unused int) {
	_ = unused
}

func ExportedUnused(x int) {}

// The three families the SSA half of the check reports.

// `result N is always X`: every return gives the second result the same
// constant. gitea's `getStorageSectionByType` is this shape.
func sectionByType(name string) (string, secKind, error) {
	if name == "" {
		return "", 0, errNotFound
	}
	if name == "x" {
		return name, 0, nil
	}
	return "", 0, errNotFound
}

// `result N is never used`: no call site reads the first result, and at least
// two ignore it.
func saveBlob(data string) (string, error) {
	if data == "" {
		return "", errNotFound
	}
	return data + "!", nil
}

func useSaveBlob() error {
	_, err := saveBlob("a")
	if err != nil {
		return err
	}
	_, err = saveBlob("b")
	return err
}

// `param always receives X`: four call sites, all passing the same constant.
// Reported even though the body uses it.
func xmlResponse(status int, obj string) string {
	if status > 0 {
		return obj
	}
	return ""
}

func useXML() string {
	return xmlResponse(statusOK, "a") + xmlResponse(statusOK, "b") +
		xmlResponse(statusOK, "c") + xmlResponse(statusOK, "d")
}

func useSectionByType(typ string) (string, secKind, error) {
	sec, kind, err := sectionByType(typ)
	if sec != "" || err != nil {
		return sec, kind, err
	}
	return "", 0, nil
}

func runFn(f func()) {}

// Upstream names a function literal after its enclosing function, the way
// go/ssa does — `litIIFE$1`, and `$1$1` for one nested inside another. guff
// printed the placeholder "<func literal>", which golangci-lint can never
// emit, so every such finding was a guaranteed mismatch; nothing caught it
// because this fixture had no literal in it at all.
func litIIFE() {
	_ = func(used int, unused string) int { return used + 1 }(1, "x")
}

// Silent in both: a literal passed as an argument has its signature fixed by
// the callee.
func litAsArg() {
	runFn(func() {})
}

// Silent in both: `go` and `defer` fix the signature the same way.
func litGoDefer() {
	go func(unusedGo int) {}(1)
	defer func(unusedDefer int) {}(2)
}
