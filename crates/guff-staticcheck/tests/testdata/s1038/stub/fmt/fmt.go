package fmt

type fmtError string

func (e fmtError) Error() string { return string(e) }

func Print(a ...interface{}) (int, error)                                 { return 0, fmtError("") }
func Printf(format string, a ...interface{}) (int, error)                 { return 0, fmtError("") }
func Println(a ...interface{}) (int, error)                               { return 0, fmtError("") }
func Sprint(a ...interface{}) string                                      { return "" }
func Sprintf(format string, a ...interface{}) string                      { return "" }
func Sprintln(a ...interface{}) string                                    { return "" }
func Errorf(format string, a ...interface{}) error                        { return fmtError("") }
func Fprint(w interface{}, a ...interface{}) (int, error)                 { return 0, fmtError("") }
func Fprintf(w interface{}, format string, a ...interface{}) (int, error) { return 0, fmtError("") }
func Fprintln(w interface{}, a ...interface{}) (int, error)               { return 0, fmtError("") }
