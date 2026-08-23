package bytes

type Buffer struct{}

func (b *Buffer) Write(p []byte) (int, error)   { return 0, nil }
func (b *Buffer) WriteString(s string) (int, error) { return 0, nil }
func (b *Buffer) String() string                { return "" }
