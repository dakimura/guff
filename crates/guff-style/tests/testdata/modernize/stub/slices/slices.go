package slices

func Sort[S ~[]E, E cmpOrdered](x S) {}
func Contains[S ~[]E, E comparable](s S, v E) bool { return false }
func Backward[S ~[]E, E any](s S) []E { return nil }

type cmpOrdered interface {
	~int | ~string
}
