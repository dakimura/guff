package strconv

func ParseComplex(s string, bitSize int) (complex128, error) {
	var c complex128
	var err error
	return c, err
}

func ParseFloat(s string, bitSize int) (float64, error) {
	var f float64
	var err error
	return f, err
}

func ParseInt(s string, base int, bitSize int) (int64, error) {
	var i int64
	var err error
	return i, err
}

func ParseUint(s string, base int, bitSize int) (uint64, error) {
	var u uint64
	var err error
	return u, err
}

func FormatComplex(c complex128, fmt byte, prec, bitSize int) string {
	return ""
}

func FormatFloat(f float64, fmt byte, prec, bitSize int) string {
	return ""
}

func FormatInt(i int64, base int) string {
	return ""
}

func FormatUint(i uint64, base int) string {
	return ""
}

func AppendFloat(dst []byte, f float64, fmt byte, prec, bitSize int) []byte {
	return dst
}

func AppendInt(dst []byte, i int64, base int) []byte {
	return dst
}

func AppendUint(dst []byte, i uint64, base int) []byte {
	return dst
}
