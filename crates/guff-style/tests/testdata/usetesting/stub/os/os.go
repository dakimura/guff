package os

func MkdirTemp(dir, pattern string) (string, error) { return "", nil }
func CreateTemp(dir, pattern string) (*File, error) { return nil, nil }
func Chdir(dir string) error                        { return nil }
func TempDir() string                               { return "" }
func Setenv(key, value string) error                { return nil }

type File struct{}
