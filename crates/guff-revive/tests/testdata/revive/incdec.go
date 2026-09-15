// Package incdec is `increment-decrement` over every left-hand side.
//
// Upstream's message is
//
//	fmt.Sprintf("should replace %s with %s%s", file.Render(as),
//		file.Render(as.Lhs[0]), suffix)
//
// so it renders whatever the LHS is. guff accepted an `Ident` and nothing
// else, which left out a field (`m.Views += 1`), an index, a map index and a
// parenthesised deref — and with it most of where `+= 1` is actually written,
// since a method updating its own state cannot use a bare identifier.
// grafana/photoprism's `internal/entity` had three.
package incdec

type M struct {
	Views int
	Xs    []int
	Sub   *M
}

func (m *M) Bump(i int, xs []int, mm map[string]int) {
	n := 0
	n += 1            // FINDING: n += 1 -> n++
	m.Views += 1      // FINDING: field
	m.Sub.Views += 1  // FINDING: nested field
	xs[i] += 1        // FINDING: index by variable
	m.Xs[0] -= 1      // FINDING: index by constant, and the `--` suffix
	mm["k"] += 1      // FINDING: map index
	(*m).Views += 1   // FINDING: parenthesised deref
	n -= 1            // FINDING
	n += 2            // silent: only 1 counts
	m.Views, n = n, 1 // silent: two left-hand sides
	_ = n
}
