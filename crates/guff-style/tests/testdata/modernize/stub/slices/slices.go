package slices

func Sort[S ~[]E, E cmpOrdered](x S) {}
func Contains[S ~[]E, E comparable](s S, v E) bool { return false }

type cmpOrdered interface {
	~int | ~string
}
