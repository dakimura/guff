package unsafe
type Pointer *struct{}
func String(ptr *byte, len IntegerType) string { return "" }
func StringData(str string) *byte { return nil }
func Slice(ptr *ArbitraryType, len IntegerType) []ArbitraryType { return nil }
func SliceData(slice []ArbitraryType) *ArbitraryType { return nil }
type ArbitraryType int
type IntegerType int
