package slices

func Sort[S ~[]E, E cmpOrdered](x S) {}

type cmpOrdered interface {
	~int | ~string
}
