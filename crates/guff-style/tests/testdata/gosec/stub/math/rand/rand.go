package rand
func New(src interface{}) interface{} { return nil }
func Read(p []byte) (n int, err error) { return 0, nil }
func ExpFloat64() float64 { return 0 }
func Float32() float32 { return 0 }
func Float64() float64 { return 0 }
func Int() int { return 0 }
func Int31() int32 { return 0 }
func Int31n(n int32) int32 { return 0 }
func Int63() int64 { return 0 }
func Int63n(n int64) int64 { return 0 }
func Intn(n int) int { return 0 }
func NormFloat64() float64 { return 0 }
func Perm(n int) []int { return nil }
func Shuffle(n int, swap func(i, j int)) {}
func Uint32() uint32 { return 0 }
func Uint64() uint64 { return 0 }
