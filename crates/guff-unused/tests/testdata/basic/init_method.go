// A *method* named `init` is a package's init function as far as `unused` is
// concerned, and so is everything it reaches.
//
//	if token.IsExported(decl.Name.Name) && g.opts.ExportedIsUsed {
//		if decl.Recv == nil {
//			// (1.2) packages use exported functions
//			g.use(obj, nil)
//		}
//	} else if decl.Name.Name == "init" {
//		// (1.5) packages use init functions
//		g.use(obj, nil)
//	} else if decl.Name.Name == "main" && g.pkg.Name() == "main" {
//
// The `decl.Recv == nil` guard is on the exported branch and on no other, so
// `func (d *Diff) init()` takes the `init` branch. opentofu's
// `internal/legacy/tofu/diff.go:226` is one, and the methods it calls are
// kept alive with it.
package initmethod

type Diff struct{ Modules []*ModuleDiff }

type ModuleDiff struct{ Path string }

// silent — a method named `init`, called from nowhere.
func (d *Diff) init() {
	for _, m := range d.Modules {
		m.reset()
	}
}

// silent — reachable only from `init`, which is a root.
func (m *ModuleDiff) reset() { m.Path = "" }

// fires — the same body under a name that is not `init`.
func (d *Diff) initNever() {
	for _, m := range d.Modules {
		m.reset()
	}
}

func UseDiff() *Diff { return &Diff{} }
