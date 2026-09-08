// Package varinit is unparam's "always receives" over call sites that live in
// a package-level variable initialiser.
//
// Upstream collects call sites with `ssautil.AllFunctions`, which reaches the
// synthesized package `init` and the `func` literals under it (it names them
// `init$1`, `init$2`, …). guff collected them from `src_funcs_with_methods()`,
// which starts from *named* functions, so every call written inside
// `var _ = ...` was invisible.
//
// That is Ginkgo's shape, and podman v6.1.0 is where it showed: its machine
// e2e suite is `var _ = Describe("...", func() { It("...", func() { ... }) })`
// with eight `setTimeout(time.Minute * 10)` calls inside. guff counted zero
// call sites, `alwaysReceivedConst`'s "fewer than four" guard returned in
// silence, and what surfaced instead was nolintlint calling the file's
// `//nolint: unparam` directive unused.
//
// unparam wants at least four call sites, so `insideInit` gets exactly four
// and `outsideInit` — the control, called from an ordinary function — gets
// four of its own.
package varinit

// Stands in for Ginkgo's Describe/It: a function taking a closure, called from
// a package-level variable initialiser. Both record `text` so that they are
// not themselves unparam findings, which would say nothing about call sites.
var names []string

func describe(text string, body func()) bool {
	names = append(names, text)
	body()
	return true
}

func it(text string, body func()) {
	names = append(names, text)
	body()
}

type builder struct {
	timeout int
	name    string
}

// fires — all four call sites are under the package initialiser.
func (b *builder) insideInit(timeout int) *builder {
	b.timeout = timeout
	return b
}

// fires — the control, with its call sites in an ordinary function.
func (b *builder) outsideInit(timeout int) *builder {
	b.timeout = timeout
	return b
}

func (b *builder) setName(n string) *builder {
	b.name = n
	return b
}

func (b *builder) run() error { return nil }

var _ = describe("suite", func() {
	it("a", func() {
		b := &builder{}
		_ = b.setName("a").insideInit(600).run()
	})
	it("b", func() {
		b := &builder{}
		_ = b.setName("b").insideInit(600).run()
	})
	it("c", func() {
		b := &builder{}
		_ = b.setName("c").insideInit(600).run()
	})
	it("d", func() {
		b := &builder{}
		_ = b.setName("d").insideInit(600).run()
	})
})

func drive() {
	b := &builder{}
	_ = b.setName("a").outsideInit(600).run()
	_ = b.setName("b").outsideInit(600).run()
	_ = b.setName("c").outsideInit(600).run()
	_ = b.setName("d").outsideInit(600).run()
}
