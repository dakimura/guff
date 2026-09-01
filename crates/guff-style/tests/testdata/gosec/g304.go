// Package g304 is gosec's G304: a file read whose path is not a compile-time
// constant.
//
// "The argument is a variable" is only the first branch. The rule also keeps
// two side maps — variables assigned from `filepath.Clean` (or `Rel`, or
// `EvalSymlinks`) and variables assigned from `filepath.Join` — and they are
// filled in AST visit order, so a call that comes before its assignment does
// not see it. Every function below is one call, marked `// fires` or
// `// silent`.
package g304

import (
	"io/ioutil"
	"os"
	"path"
	"path/filepath"
)

const constPath = "/etc/passwd"

// fires — a parameter is not a constant.
func VarPath(p string) ([]byte, error) { return os.ReadFile(p) }

// silent — a string literal.
func Literal() ([]byte, error) { return os.ReadFile("/etc/passwd") }

// silent — an identifier that resolves to one.
func ConstIdent() ([]byte, error) { return os.ReadFile(constPath) }

// silent — `Clean` written inline.
func Cleaned(p string) ([]byte, error) { return os.ReadFile(filepath.Clean(p)) }

// fires — a `Join` of two variables.
func Joined(dir, name string) (*os.File, error) { return os.Open(filepath.Join(dir, name)) }

// silent — a literal base joined with a cleaned name. This pair is the reason
// `isSafeJoin` exists; either half alone is not enough.
func SafeJoin(name string) (*os.File, error) {
	return os.Open(filepath.Join("/base", filepath.Clean(name)))
}

// fires — the same `Join`, through a variable.
func JoinedVar(dir, name string) (*os.File, error) {
	p := filepath.Join(dir, name)

	return os.Open(p)
}

// silent — a variable assigned from `Clean`.
func CleanedVar(p string) (*os.File, error) {
	q := filepath.Clean(p)

	return os.Open(q)
}

// fires — concatenation that mentions a variable.
func Concat(name string) (*os.File, error) { return os.Open("/base/" + name) }

// silent — concatenation of two literals.
func ConcatConst() (*os.File, error) { return os.Open("/base/" + "x") }

// fires — `Create` and `OpenFile` are on the call list too.
func Create(p string) (*os.File, error) { return os.Create(p) }

func OpenFile(p string) (*os.File, error) { return os.OpenFile(p, os.O_RDONLY, 0o600) }

// silent — `os.Stat` is not: the list is ReadFile / Open / OpenFile / Create
// plus `io/ioutil.ReadFile`, and nothing else.
func Stat(p string) (os.FileInfo, error) { return os.Stat(p) }

// silent — `Rel` is on the clean list beside `Clean`.
func RelVar(base, target string) (*os.File, error) {
	p, _ := filepath.Rel(base, target)

	return os.Open(p)
}

// silent — and so is `EvalSymlinks`.
func EvalSymlinksVar(target string) (*os.File, error) {
	p, _ := filepath.EvalSymlinks(target)

	return os.Open(p)
}

// fires — the deprecated `io/ioutil` spelling.
func IoutilRead(p string) ([]byte, error) { return ioutil.ReadFile(p) }

// fires — `path.Join` as well as `path/filepath.Join`.
func PathJoin(dir, name string) (*os.File, error) { return os.Open(path.Join(dir, name)) }

// silent — a local assigned a literal resolves.
func VarFromLiteral() (*os.File, error) {
	p := "/etc/passwd"

	return os.Open(p)
}

// silent — cleaned after being joined, and the clean set is asked first.
func JoinThenClean(dir, name string) (*os.File, error) {
	p := filepath.Join(dir, name)
	p = filepath.Clean(p)

	return os.Open(p)
}

// fires — the call comes *before* the clean, and the maps are filled in visit
// order. This is the shape that says the rule is not flow-sensitive.
func UseBeforeClean(p string) (*os.File, error) {
	f, err := os.Open(p)
	q := filepath.Clean(p)
	_ = q

	return f, err
}

// silent — every argument of the join is a constant.
func JoinConst() (*os.File, error) { return os.Open(filepath.Join("/base", "name")) }

// fires — a literal base is not enough on its own; `isSafeJoin` wants a cleaned
// argument beside it.
func JoinLiteralVar(name string) (*os.File, error) {
	p := filepath.Join("/base", name)

	return os.Open(p)
}
