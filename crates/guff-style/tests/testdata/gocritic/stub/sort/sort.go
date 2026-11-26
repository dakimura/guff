package sort

type StringSlice []string
type IntSlice []int
type Float64Slice []float64

func (x StringSlice) Len() int           { return len(x) }
func (x StringSlice) Less(i, j int) bool { return x[i] < x[j] }
func (x StringSlice) Swap(i, j int)      { x[i], x[j] = x[j], x[i] }

func (x IntSlice) Len() int           { return len(x) }
func (x IntSlice) Less(i, j int) bool { return x[i] < x[j] }
func (x IntSlice) Swap(i, j int)      { x[i], x[j] = x[j], x[i] }

func (x Float64Slice) Len() int           { return len(x) }
func (x Float64Slice) Less(i, j int) bool { return x[i] < x[j] }
func (x Float64Slice) Swap(i, j int)      { x[i], x[j] = x[j], x[i] }

func Slice(x any, less func(i, j int) bool)       {}
func SliceStable(x any, less func(i, j int) bool) {}
func Strings(x []string)                          {}
func Ints(x []int)                                {}
func Float64s(x []float64)                        {}
