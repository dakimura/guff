//go:build go1.26

package newexpr

// intVar returns a new var whose value is i.
func intVar(i int) *int { return &i }

func int64Var(i int64) *int64 { return &i }

func stringVar(s string) *string { return &s }

func varOf[T any](x T) *T { return &x }

//go:fix inline
func alreadyAnnotated[T any](x T) *T { return &x }

func variadic[T any](x ...T) *[]T { return &x }

var (
	s struct {
		int
		string
	}
	_ = intVar(123)
	_ = int64Var(123)
	_ = stringVar("abc")
	_ = varOf(s)
	_ = varOf(123)
	_ = varOf(int64(123))
	_ = varOf[int](123)
	_ = varOf[int64](123)
	_ = varOf(varOf(123))
	_ = alreadyAnnotated[int](123)
	_ = variadic[int]()
)
