package rand

type Rand struct{}

func Intn(n int) int          { return 0 }
func Int() int                { return 0 }
func Float64() float64        { return 0 }
func Perm(n int) []int        { return nil }
func (r *Rand) Intn(n int) int { return 0 }
