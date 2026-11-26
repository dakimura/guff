package fmt

func Append(b []byte, a ...any) []byte                 { return b }
func Appendf(b []byte, format string, a ...any) []byte { return b }
func Appendln(b []byte, a ...any) []byte               { return b }
func Errorf(format string, a ...any) error             { return nil }
func Sprintf(format string, a ...any) string           { return "" }
