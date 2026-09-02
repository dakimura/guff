package fmt

type Stringer interface {
	String() string
}

func Printf(format string, a ...any) {}

func Sprintf(format string, a ...any) string { return "" }

func Errorf(format string, a ...any) error { return nil }

func Println(a ...any) {}
