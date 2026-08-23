package os

type FileMode uint32

const ModePerm FileMode = 0777

type File struct{}

// Only `Write`: enough to be an `io.Writer`, and deliberately *not* enough to
// be an `http.ResponseWriter`. G705's guard turns on exactly that difference.
func (f *File) Write(p []byte) (int, error) { return 0, nil }

var (
	Stdout *File
	Stderr *File
)
type FileInfo interface{}
type DirEntry interface{}

func Mkdir(name string, perm FileMode) error                         { return nil }
func MkdirAll(path string, perm FileMode) error                      { return nil }
func OpenFile(name string, flag int, perm FileMode) (*File, error)   { return nil, nil }
func Chmod(name string, mode FileMode) error                         { return nil }
func WriteFile(name string, data []byte, perm FileMode) error        { return nil }
func Create(name string) (*File, error)                              { return nil, nil }
func TempDir() string                                                { return "/tmp" }
func Remove(name string) error                                       { return nil }
func RemoveAll(path string) error                                    { return nil }
func Getenv(key string) string                                       { return "" }
func ReadFile(name string) ([]byte, error)                           { return nil, nil }
func Open(name string) (*File, error)                                { return nil, nil }
func Stat(name string) (FileInfo, error)                             { return nil, nil }
func Lstat(name string) (FileInfo, error)                            { return nil, nil }
func Rename(oldpath, newpath string) error                           { return nil }
func Chown(name string, uid, gid int) error                          { return nil }
func Environ() []string                                              { return nil }
func StartProcess(name string, argv []string, attr interface{}) (*Process, error) {
	return nil, nil
}

type Process struct{}

// Args is a *source* for every taint rule: the key is "os.Args", matched on the
// package-level variable itself (gosec's `*ssa.Global` arm).
var Args []string

const O_RDONLY = 0

