package generic

// ST1016 never reports a generic type. Upstream skips "embedded methods" with
// `typeutil.Dereference(recv.Type()) != T.Type()`, a pointer comparison, and a
// method of `type G[T any]` has an *instantiation* `G[T']` for a receiver —
// never the origin — so every method is skipped. mimir's
// `rangevectorsplitting.CachedSplit[T]` names its receivers `p` twice and `c`
// nine times. Measured against golangci-lint 2.12.2: only `Control` reports.

// Silent: pointer receivers, mixed names.
type Gen[T any] struct{ v T }

func (p *Gen[T]) A() {}
func (c *Gen[T]) B() {}

// Silent: value receivers.
type GenV[T any] struct{ v T }

func (p GenV[T]) A() {}
func (c GenV[T]) B() {}

// Silent: two type parameters.
type Gen2[K comparable, V any] struct{}

func (a *Gen2[K, V]) A() {}
func (b *Gen2[K, V]) B() {}

// Silent: the type parameter renamed per method.
type GenP[T any] struct{}

func (p *GenP[T]) A() {}
func (q *GenP[U]) B() {}

// Silent: consistent anyway.
type GenOK[T any] struct{}

func (g *GenOK[T]) A() {}
func (g *GenOK[T]) B() {}

// Reported: the non-generic control, so the file is not silent by default.
type Control struct{}

func (p *Control) A() {}
func (q *Control) B() {}
