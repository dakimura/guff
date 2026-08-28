package unexported

// Neither of these is reported: start-with-name is pinned
// `include-tests: false` by golangci-lint, so the whole file is invisible to
// it whatever include-unexported says.
func TestSomething() {}

// Also not the symbol name.
func testLocal() {}
