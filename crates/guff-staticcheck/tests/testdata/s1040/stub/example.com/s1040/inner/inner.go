// Package inner exists so the message has an imported type to render. Upstream
// prints `report.Render(pass, expr.Type)` — the source expression — so this is
// `inner.Msg`, not the resolved type's full import path.
package inner

type Msg interface{ M() }

type Box struct{}

func (Box) Get() Msg { return nil }
