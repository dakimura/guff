package strconv

func Atoi(s string) (int, error) { return 0, nil }

// ParseInt / ParseUint: G115 reads the `bitSize` argument to bound the parsed
// value (gosec ComputeRange's `*ssa.Extract` arm).
func ParseInt(s string, base int, bitSize int) (int64, error) { return 0, nil }

func ParseUint(s string, base int, bitSize int) (uint64, error) { return 0, nil }

func Itoa(i int) string                                    { return "" }
func Quote(s string) string                                { return s }
func ParseFloat(s string, bitSize int) (float64, error)    { return 0, nil }
func ParseBool(str string) (bool, error)                   { return false, nil }
func FormatInt(i int64, base int) string                   { return "" }
func FormatUint(i uint64, base int) string                 { return "" }
func FormatFloat(f float64, fmtc byte, prec, bitSize int) string { return "" }
