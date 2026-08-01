package filepath

import "os"

func Walk(root string, fn func(path string, info os.FileInfo, err error) error) error {
	return nil
}

func WalkDir(root string, fn func(path string, d os.DirEntry, err error) error) error {
	return nil
}
