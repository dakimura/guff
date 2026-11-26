package slices

func Sort[S ~[]E, E cmpOrdered](x S) {}
func Contains[S ~[]E, E comparable](s S, v E) bool { return false }
func ContainsFunc[S ~[]E, E any](s S, f func(E) bool) bool { return false }
func Backward[S ~[]E, E any](s S) []E { return nil }
func Delete[S ~[]E, E any](s S, i, j int) S { return s }

type cmpOrdered interface {
	~int | ~string
}

func IndexFunc[S ~[]E, E any](s S, f func(E) bool) int { return -1 }
