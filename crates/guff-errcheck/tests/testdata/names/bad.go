// Package names covers how the finding *names* the callee.
//
// golangci prints `cmp.Or(SelectorName, FuncName)` — the selector as written,
// falling back to the qualified name — or `FuncName` alone under
// `errcheck.verbose`. Both are empty when the callee is not a selector, and
// then the message carries no name at all.
package names

type writer struct{ inner *writer }

func (w writer) Emit() error   { return nil }
func (w *writer) Flush() error { return nil }

type emitter interface {
	Emit() error
}

func newWriter() writer { return writer{} }

func plain() error { return nil }

func generic[T any]() error { return nil }

var pkgLevel writer

func Shapes() {
	// No selector at all: the short form.
	plain()
	generic[int]()
	fn := plain
	fn()

	// A selector spelled as a chain of identifiers: printed verbatim.
	var w writer
	w.Emit()
	pkgLevel.Emit()
	w.inner.Flush()

	// A selector whose receiver is not an identifier chain: SelectorName is
	// empty and the qualified name is printed instead.
	newWriter().Emit()
	(&w).Flush()

	// An interface method: the qualified name names the interface.
	var e emitter = w
	e.Emit()
}
