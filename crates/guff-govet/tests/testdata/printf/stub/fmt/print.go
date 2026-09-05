package fmt

type Stringer interface {
	String() string
}

func Printf(format string, a ...any) {}

func Sprintf(format string, a ...any) string { return "" }

func Errorf(format string, a ...any) error { return nil }

func Println(a ...any) {}

// State and Formatter, so the `isFormatter` test has something to find. A type
// whose `Format` takes anything else is not a Formatter, which is why the
// signature is spelled out rather than stubbed as `any`.
type State interface {
	Write(b []byte) (n int, err error)
	Width() (wid int, ok bool)
	Precision() (prec int, ok bool)
	Flag(c int) bool
}

type Formatter interface {
	Format(f State, verb rune)
}
