// `unused` rule (2.1): "named types use exported methods".
//
//	if g.opts.ExportedIsUsed {
//		for m := range ms.Methods() {
//			if token.IsExported(m.Obj().Name()) {
//				// (2.1) named types use exported methods
//				g.readSelection(m, named)
//			}
//		}
//	}
//
// `readSelection(m, named)` is an edge **from the type**: the exported method
// is used when the type is. Upstream's own rule list says the opposite —
// "(8.1) Exported methods on concrete types are always marked as used" — and
// the code is what runs. Reading it as a root makes the method keep its
// receiver alive, so a type nothing references disappears from the report
// along with all of its methods. opentofu's `basicComponentFactory` (four
// exported methods, never constructed, no `var _`) is that shape.
package exportedmethod

// fires — the type and its exported method, because nothing uses the type.
type withExported struct{ n int }

func (w *withExported) Only() int { return w.n }

// fires — the unexported case was already right, and is here so a fix that
// swings the other way is visible.
type withUnexported struct{ n int }

func (w *withUnexported) only() int { return w.n }

// silent — the type is used, so its exported method is too.
type UsedType struct{ n int }

func (u *UsedType) Only() int { return u.n }

func UseIt() *UsedType { return &UsedType{} }
