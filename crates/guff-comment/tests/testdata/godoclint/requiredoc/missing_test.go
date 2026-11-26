package requiredoc

// require-doc is pinned `include-tests: true` — the opposite of
// require-pkg-doc's `false`. These are reported.

const FooTest = 0 //foo:bar

func (*TFooNG) FooFooTest() {} //foo:bar

const fooTest = 0 //foo:bar
