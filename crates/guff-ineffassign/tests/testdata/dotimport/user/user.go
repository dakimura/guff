// Upstream keys its variable table on `*ast.Object`, which the parser fills
// in — so a name it cannot resolve within the file has a nil `Obj` and is
// never tracked. A dot-imported variable is exactly that. guff resolved
// identifiers through the type checker, which does know the name, and reported
// an assignment to another package's variable as an ineffectual assignment to
// a local (velero's `ReportData`, reached through
// `. "github.com/vmware-tanzu/velero/test"`).
package user

import (
	. "example.com/ineffassign/dotimport/shared"
)

// Silent: `Shared` belongs to another package.
func assignDotImported() error {
	Shared = &Report{N: 1}

	return nil
}

// Silent: twice over, the first of which would be ineffectual for a local.
func assignDotImportedTwice() {
	Shared = &Report{N: 1}
	Shared = &Report{N: 2}
}

// Reported: a local that shadows the dot-imported name is still a local.
func shadowsDotImported() {
	Shared := &Report{N: 1}
	Shared = &Report{N: 2}
	_ = Shared
}
