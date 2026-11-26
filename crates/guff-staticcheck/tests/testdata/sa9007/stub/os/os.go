package os

func RemoveAll(path string) error { return nilErr{} }

func TempDir() string { return "/tmp" }

type nilErr struct{}

func (nilErr) Error() string { return "" }
