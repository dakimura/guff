package p

import "context"

// `isWithinLoop` — the badly named predicate that decides whether the variable
// being reassigned was *declared inside the enclosing node* — is asked with the
// node the report would be attributed to, and that node is a `FuncDecl` for
// every assignment written in a plain function body. A `context.Context`
// parameter's scope is the function body, so it is inside its own declaration
// and upstream leaves it alone; only a variable declared somewhere wider is a
// finding. guff had no `FuncDecl` arm in the span helper, so the predicate
// answered "not within" for all of them and reported the lot.

var pkgCtx context.Context

type value struct{ ctx context.Context }

// Silent: the parameter is declared by this very function.
func ParamAtTopLevel(ctx context.Context) {
	ctx = context.WithValue(ctx, "k", "v")
	_ = ctx
}

// Silent: so is a local.
func LocalAtTopLevel() {
	var ctx context.Context
	ctx = context.WithValue(ctx, "k", "v")
	_ = ctx
}

// Silent: the statement-list recursion reaches into `if`, `switch` and
// `select`, and the enclosing node is still the declaration.
func ParamInIf(ctx context.Context, b bool) {
	if b {
		ctx = context.WithValue(ctx, "k", "v")
	}
	_ = ctx
}

func ParamInSwitch(ctx context.Context, n int) {
	switch n {
	case 1:
		ctx = context.WithValue(ctx, "k", "v")
	}
	_ = ctx
}

func ParamInSelect(ctx context.Context, ch chan struct{}) {
	select {
	case <-ch:
		ctx = context.WithValue(ctx, "k", "v")
	}
	_ = ctx
}

// Reported: a package-level variable is declared outside the function, so the
// predicate really is being asked and really can say no.
func PackageVarAtTopLevel() {
	pkgCtx = context.WithValue(pkgCtx, "k", "v")
	_ = pkgCtx
}

// Silent: the root identifier of `v.ctx` is the receiver, declared here.
func (v value) OwnField() {
	v.ctx = context.WithValue(v.ctx, "k", "v")
}

// Silent: and of a local struct value.
func LocalStructField() {
	var v value
	v.ctx = context.WithValue(v.ctx, "k", "v")
	_ = v
}

// Reported as a loop: inside a `for` the enclosing node is the loop, and the
// parameter is not declared there.
func ParamInLoop(ctx context.Context, n int) {
	for range n {
		ctx = context.WithValue(ctx, "k", "v")
		_ = ctx
	}
}

// Reported as a loop: so is the package variable.
func PackageVarInLoop(n int) {
	for range n {
		pkgCtx = context.WithValue(pkgCtx, "k", "v")
		_ = pkgCtx
	}
}

// Silent: a variable declared inside the loop body is within it.
func LocalInLoop(n int) {
	for range n {
		var ctx context.Context
		ctx = context.WithValue(ctx, "k", "v")
		_ = ctx
	}
}

// Silent: the literal's own parameter is declared by the literal.
func LitOwnParam() func(context.Context) {
	return func(ctx context.Context) {
		ctx = context.WithValue(ctx, "k", "v")
		_ = ctx
	}
}
