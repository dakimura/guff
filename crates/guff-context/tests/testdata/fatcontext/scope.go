package fatcontext

import "context"

// `isWithinLoop` decides whether the variable being reassigned was declared
// inside the node the report would be attributed to — and for an assignment in
// a plain function body that node is the `FuncDecl`. A parameter's scope is the
// function body, so it lies inside its own declaration and upstream says
// nothing; only a variable declared somewhere wider is a finding.

var pkgCtx context.Context

type value struct{ ctx context.Context }

func paramAtTopLevel(ctx context.Context) {
	ctx = context.WithValue(ctx, "k", "v")
	_ = ctx
}

func localAtTopLevel() {
	var ctx context.Context
	ctx = context.WithValue(ctx, "k", "v")
	_ = ctx
}

func paramInIf(ctx context.Context, b bool) {
	if b {
		ctx = context.WithValue(ctx, "k", "v")
	}
	_ = ctx
}

func paramInSwitch(ctx context.Context, n int) {
	switch n {
	case 1:
		ctx = context.WithValue(ctx, "k", "v")
	}
	_ = ctx
}

func paramInSelect(ctx context.Context, ch chan struct{}) {
	select {
	case <-ch:
		ctx = context.WithValue(ctx, "k", "v")
	}
	_ = ctx
}

// The one control at the top level: a package variable is declared outside.
func packageVarAtTopLevel() {
	pkgCtx = context.WithValue(pkgCtx, "k", "v")
	_ = pkgCtx
}

func (v value) ownField() {
	v.ctx = context.WithValue(v.ctx, "k", "v")
}

func localStructField() {
	var v value
	v.ctx = context.WithValue(v.ctx, "k", "v")
	_ = v
}

func paramInLoop(ctx context.Context, n int) {
	for i := 0; i < n; i++ {
		ctx = context.WithValue(ctx, "k", "v")
		_ = ctx
	}
}

func packageVarInLoop(n int) {
	for i := 0; i < n; i++ {
		pkgCtx = context.WithValue(pkgCtx, "k", "v")
		_ = pkgCtx
	}
}

func localInLoop(n int) {
	for i := 0; i < n; i++ {
		var ctx context.Context
		ctx = context.WithValue(ctx, "k", "v")
		_ = ctx
	}
}

func litOwnParam() func(context.Context) {
	return func(ctx context.Context) {
		ctx = context.WithValue(ctx, "k", "v")
		_ = ctx
	}
}
