package scope

const TestOnly = "test-only-const"

func helper() {
	a := "shared-value"
	b := "shared-value"
	_, _ = a, b
	c := "test-only-const"
	_ = c
}
