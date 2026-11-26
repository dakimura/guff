package fmt

func Println(a ...interface{}) (n int, err error) { return 0, nil }
func Print(a ...interface{}) (n int, err error)   { return 0, nil }
func Printf(format string, a ...interface{}) (n int, err error) {
	return 0, nil
}
func Sprint(a ...interface{}) string                 { return "" }
func Sprintf(format string, a ...interface{}) string { return "" }
func Sprintln(a ...interface{}) string               { return "" }
func Fprint(w interface{}, a ...interface{}) (n int, err error) {
	return 0, nil
}
func Fprintf(w interface{}, format string, a ...interface{}) (n int, err error) {
	return 0, nil
}
func Fprintln(w interface{}, a ...interface{}) (n int, err error) {
	return 0, nil
}
func Errorf(format string, a ...interface{}) error { return nil }
