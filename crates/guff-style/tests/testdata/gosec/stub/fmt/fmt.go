package fmt

import "io"

func Fprintf(w io.Writer, format string, a ...any) (int, error) { return 0, nil }
func Fprint(w io.Writer, a ...any) (int, error)                 { return 0, nil }
func Fprintln(w io.Writer, a ...any) (int, error)               { return 0, nil }
func Sprintf(format string, a ...any) string                    { return "" }
