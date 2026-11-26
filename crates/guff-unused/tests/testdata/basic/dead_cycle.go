package deadcycle

// `recompileAll` is called — but only from two methods nothing calls, so
// honnef's graph never reaches it from a root and all three are unused. dapr
// silences exactly this shape in
// `pkg/runtime/hotreload/reconciler/workflowaccesspolicies.go`.
type policies struct{ appID string }

func New(id string) *policies { return &policies{appID: id} }

func (p *policies) recompileAll() { _ = p.appID }

func (p *policies) update() { p.recompileAll() }

func (p *policies) delete() { p.recompileAll() }

// A live chain from an exported root stays live all the way down.
func Reload(p *policies) { p.reachable() }

func (p *policies) reachable() { p.alsoReachable() }

func (p *policies) alsoReachable() { _ = p.appID }
