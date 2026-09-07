package main

// The loop shapes, and what the back edge can and cannot carry.
//
// Upstream has no AST layer here at all: `hasUse` walks the IR and a value whose
// only referrers are phis with no real use is dead. guff's hybrid SSA drops some
// loads, so `ssa_unused_but_ast_read` suppresses a report when the AST shows a
// read — and its loop half used to say "a read anywhere inside an enclosing loop
// means the value is live". That is only true when the back edge can reach the
// read. A `:=` at the top of the body redefines first, so the reads after it are
// this iteration's, not the previous one's, and the value assigned below them is
// dead. hashicorp/packer's `hcl2template/types.packer_config.go:636` is shape 1.

type Diags []string

func (d Diags) HasErrors() bool { return len(d) > 0 }

func start(i int) (int, Diags)  { return i, nil }
func decode(i int) (int, Diags) { return i, nil }

func use(...any) {}

// 1. reported: the body redefines before it reads
func loopRedefineThenRead(n int) Diags {
	var diags Diags
	for i := 0; i < n; i++ {
		pp, moreDiags := start(i)
		diags = append(diags, moreDiags...)
		if moreDiags.HasErrors() {
			continue
		}
		flat, moreDiags := decode(pp)
		use(flat)
	}
	return diags
}

// 2. reported: the same without a loop at all
func straightLine(n int) Diags {
	var diags Diags
	pp, moreDiags := start(n)
	diags = append(diags, moreDiags...)
	flat, moreDiags := decode(pp)
	use(flat)
	return diags
}

// 3. silent: the read comes before any redefinition, so the back edge does
// carry the value to it
func loopReadBeforeRedef(n int) {
	acc := 0
	for i := 0; i < n; i++ {
		use(acc)
		acc = acc + i
	}
}

// 4. silent: the redefinition is inside a nested `if` and may not run, so it
// cannot break the back edge
func loopGuardedRedef(n int, cond bool) {
	acc := 0
	for i := 0; i < n; i++ {
		if cond {
			acc = 0
		}
		use(acc)
		acc = acc + i
	}
}

// 5. reported: a range loop is the same shape as 1
func rangeRedefineThenRead(xs []int) Diags {
	var diags Diags
	for _, x := range xs {
		pp, moreDiags := start(x)
		diags = append(diags, moreDiags...)
		flat, moreDiags := decode(pp)
		use(flat)
	}
	return diags
}

// 6. silent: the last value written in the body is read after the loop
func loopPlainRedef(n int) {
	acc := 0
	for i := 0; i < n; i++ {
		acc = i
		use(acc)
		acc = i * 2
	}
	use(acc)
}
