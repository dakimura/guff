package filepath

import "os"

func Walk(root string, fn func(path string, info os.FileInfo, err error) error) error {
	return nil
}

func WalkDir(root string, fn func(path string, d os.DirEntry, err error) error) error {
	return nil
}

func Clean(path string) string { return path }

func ToSlash(path string) string { return path }

func Join(elem ...string) string { return "" }

func Base(path string) string                       { return path }
func Abs(path string) (string, error)               { return path, nil }
func Rel(basepath, targpath string) (string, error) { return "", nil }

func EvalSymlinks(path string) (string, error) { return path, nil }
