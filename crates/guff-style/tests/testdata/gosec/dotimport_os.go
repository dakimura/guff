// Package dotimportos is the os half of gosec's "match the receiver, not the
// callee" property, measured through a dot import.
//
// `CallList.ContainsPkgCallExpr` asks `GetCallInfo` for the receiver: a plain
// identifier call — which is what a dot-imported function is — answers with the
// *analysed package's own name* (`ctx.Pkg.Name()`, here "dotimportos"), and
// `GetImportPath` finds no import by that name, so the call matches nothing.
// A port that resolves the callee's declaring package instead sees `os.Open`
// and reports every function below.
//
// Both spellings of `os` are imported on purpose: the qualified one gives each
// dot-imported shape a control on the same page, and `os.Create(TempDir()+…)`
// is the only way to reach G303's *argument* list with a dot-imported call.
package dotimportos

import (
	"os"
	. "os"
)

// silent — dot-imported, so `GetCallInfo` never says "os".
func DotOpen(p string) (*File, error) { return Open(p) }

func DotCreate(p string) (*File, error) { return Create(p) }

func DotOpenFile(p string) (*File, error) { return OpenFile(p, O_RDONLY, 0o600) }

func DotReadFile(p string) ([]byte, error) { return ReadFile(p) }

// fires — the same five calls, package-qualified. G304 is still a G304.
func QualifiedOpen(p string) (*os.File, error) { return os.Open(p) }

func QualifiedCreate(p string) (*os.File, error) { return os.Create(p) }

func QualifiedOpenFile(p string) (*os.File, error) { return os.OpenFile(p, os.O_RDONLY, 0o600) }

func QualifiedReadFile(p string) ([]byte, error) { return os.ReadFile(p) }

// silent for G303 — `os.Create` is on G303's call list and the argument is a
// concatenation, but `findTempDirArgs` walks it down to a call it has to
// recognise as `os.TempDir`, and a dot-imported `TempDir()` is not that call.
// (It is a G304 either way: the argument mentions a variable.)
func DotTempDirArg(name string) (*os.File, error) { return os.Create(TempDir() + "/" + name) }

// fires for G303 — the same shape with the argument call qualified.
func QualifiedTempDirArg(name string) (*os.File, error) {
	return os.Create(os.TempDir() + "/" + name)
}
