package os

type FileMode uint32

const ModePerm FileMode = 0777

type File struct{}

func Mkdir(name string, perm FileMode) error                         { return nil }
func MkdirAll(path string, perm FileMode) error                      { return nil }
func OpenFile(name string, flag int, perm FileMode) (*File, error)   { return nil, nil }
func Chmod(name string, mode FileMode) error                         { return nil }
func WriteFile(name string, data []byte, perm FileMode) error        { return nil }
func Create(name string) (*File, error)                              { return nil, nil }
func TempDir() string                                                { return "/tmp" }
