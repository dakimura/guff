// Package nosec is the `#nosec` fixture, and the position fixture for the
// four AST shapes G101 reports on.
//
// Two things had no fixture at all before this file, and both were wrong:
//
//  1. Upstream builds an `ast.CommentMap` and suppresses the **line range of
//     the node the comment is attached to** (`analyzer.go`'s
//     `updateIgnoredRulesForNode` → `ignores.add`), and a finding matches an
//     ignore when *either* range contains the other (`ignores.get`). So a
//     directive written *inside* a composite literal suppresses a finding
//     reported several lines above it, at the literal's own start. guff only
//     looked backwards from the reported line, so velero's
//     `pkg/install/daemonset.go:198` was reported despite its directive.
//
//  2. `ctx.NewIssue(node, …)` takes the position from `node.Pos()`. For a
//     `*ast.CompositeLit` that is the type expression, not the `{`; for a
//     `*ast.BinaryExpr` it is `X.Pos()`, not the operator.
package nosec

type creds struct {
	Password string
	Token    string
}

// ---- ValueSpec ----------------------------------------------------------

const secretConst = "AZURE_STORAGE_KEY" // reported

const nosecConst = "AZURE_STORAGE_KEY" // #nosec

var secretVar = "AZURE_STORAGE_KEY" // reported

var nosecVar = "AZURE_STORAGE_KEY" // #nosec

// A directive naming a *different* rule must not suppress G101.
const passwordOtherRule = "AZURE_STORAGE_KEY" // #nosec G102  (reported)

const passwordNamedRule = "AZURE_STORAGE_KEY" // #nosec G101

// #nosec G101
var passwordAbove = "AZURE_STORAGE_KEY"

// #nosec G102
var passwordAboveOther = "AZURE_STORAGE_KEY" // reported

// ---- AssignStmt ---------------------------------------------------------

func assign() {
	secret := "AZURE_STORAGE_KEY" // reported
	_ = secret
}

// A trailing directive inside a function body. guff honoured package-level
// `const`/`var` and missed this one.
func assignNosec() {
	secret := "AZURE_STORAGE_KEY" // #nosec
	_ = secret
}

func assignAbove() {
	// #nosec G101
	password := "AZURE_STORAGE_KEY"
	_ = password
}

// ---- CompositeLit -------------------------------------------------------

// velero's exact shape: the directive sits between two fields of the literal,
// and the finding is reported on the literal's first line.
func veleroShape() *creds {
	return &creds{
		Token: "x",
		// #nosec G101 -- a Secret resource name, not a credential
		Password: "AZURE_STORAGE_KEY",
	}
}

func veleroShapeNoDirective() *creds {
	return &creds{ // reported here, on the `creds`, not on the `{`
		Token:    "x",
		Password: "AZURE_STORAGE_KEY",
	}
}

func insideOther() *creds {
	return &creds{ // reported: the directive names another rule
		// #nosec G102
		Password: "AZURE_STORAGE_KEY",
	}
}

func insideBare() *creds {
	return &creds{
		// #nosec
		Password: "AZURE_STORAGE_KEY",
	}
}

// A directive on the literal's *closing* line still covers it: the union of
// the node's range and the comment group's.
func onClose() *creds {
	return &creds{
		Password: "AZURE_STORAGE_KEY",
	} // #nosec G101
}

func oneLine() *creds {
	return &creds{Password: "AZURE_STORAGE_KEY"} // #nosec G101
}

// A directive in another function must not reach this one.
func farAway() *creds {
	return &creds{ // reported
		Password: "AZURE_STORAGE_KEY",
	}
}

// A non-string field value: the key matches but there is nothing to report.
func nonStringValue() *creds {
	return &creds{
		Token: "x",
	}
}

// ---- BinaryExpr ---------------------------------------------------------

func equality(password string) bool {
	return password == "AZURE_STORAGE_KEY" // reported, on `password`
}

func equalitySuppressed(password string) bool {
	return password == "AZURE_STORAGE_KEY" // #nosec G101
}
