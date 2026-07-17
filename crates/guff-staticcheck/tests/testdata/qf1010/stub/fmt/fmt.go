package fmt

type Stringer interface {
	String() string
}

func Print(a ...any) (n int, err error)     { return 0, nil }
func Println(a ...any) (n int, err error)   { return 0, nil }
func Sprint(a ...any) string                { return "" }
func Sprintln(a ...any) string              { return "" }
func Fprint(w any, a ...any) (n int, err error)   { return 0, nil }
func Fprintln(w any, a ...any) (n int, err error) { return 0, nil }
