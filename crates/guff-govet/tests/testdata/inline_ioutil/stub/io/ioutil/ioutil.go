package ioutil

import "os"

//go:fix inline
func TempDir(dir, prefix string) (string, error) {
	return os.MkdirTemp(dir, prefix)
}
