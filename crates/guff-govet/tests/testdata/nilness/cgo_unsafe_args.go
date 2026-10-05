package nilness

// f21 and f22 are golang.org/x/tools v0.50.0 nilness testdata (a/a.go).
// The compiler rejects `//go:cgo_unsafe_args` outside cgo-generated code, so
// they cannot sit in a golden case; guff type-checks without the compiler.

//go:cgo_unsafe_args
func f21(ptr *int) {
	if ptr == nil {
		print(*ptr) // nope: cgo_unsafe_args means there is magic afoot that SSA cannot see
	}
}

//go:cgo_unsafe_args
func f22(ptr *int) {
	if ptr == nil {
		f := func() {
			print(*ptr) // nope: cgo_unsafe_args means there is magic afoot that SSA cannot see
		}
		f()
	}
}

// control: the same body without the directive is a finding.
func f23(ptr *int) {
	if ptr == nil {
		print(*ptr)
	}
}
