package types

type EnumSupport struct {
	toString map[uint]string
	toEnum   map[string]uint
	maxEnum  uint
}

func NewEnumSupport(toString map[uint]string) EnumSupport {
	panic("stub")
}
func (es EnumSupport) String(e uint) string {
	panic("stub")
}
func (es EnumSupport) UnmarshJSON(b []byte) (uint, error) {
	panic("stub")
}
func (es EnumSupport) MarshJSON(e uint) ([]byte, error) {
	panic("stub")
}
