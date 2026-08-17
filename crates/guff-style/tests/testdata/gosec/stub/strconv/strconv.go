package strconv

func Atoi(s string) (int, error) { return 0, nil }

// ParseInt / ParseUint: G115 reads the `bitSize` argument to bound the parsed
// value (gosec ComputeRange's `*ssa.Extract` arm).
func ParseInt(s string, base int, bitSize int) (int64, error) { return 0, nil }

func ParseUint(s string, base int, bitSize int) (uint64, error) { return 0, nil }
