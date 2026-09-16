package rand

type Rand struct{}

func IntN(n int) int             { return 0 }
func Uint64N(n uint64) uint64    { return 0 }
func Float64() float64           { return 0 }
func Perm(n int) []int           { return nil }
func (r *Rand) IntN(n int) int   { return 0 }
