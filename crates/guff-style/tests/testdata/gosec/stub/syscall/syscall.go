package syscall

func Exec(argv0 string, argv []string, envv []string) error { return nil }
func ForkExec(argv0 string, argv []string, attr interface{}) (int, error) { return 0, nil }
func StartProcess(argv0 string, argv []string, attr interface{}) (int, uintptr, error) {
	return 0, 0, nil
}
