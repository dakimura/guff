// Package forbidigopkg is the `pkg:` filter fixture. Each call is matched by
// exactly one pattern (golden cases forbidigo-pkg-no-types and
// forbidigo-pkg-types). Upstream tests `pkg` against the resolved package
// path, which is empty without analyze-types and empty for a builtin.
package forbidigopkg

import (
	"fmt"
	"os/user"
	"strings"
)

func calls() {
	fmt.Println("x")         // pkg ^fmt$: needs types
	_ = strings.ToUpper("x") // pkg .*: matches the empty path too
	println("x")             // pkg ^$: a builtin has no package
	print("x")               // pkg .+: never, a builtin has no package
	_, _ = user.Current()    // pkg ^os/user$: needs types
	_, _ = user.Lookup("x")  // no pkg: always
}
