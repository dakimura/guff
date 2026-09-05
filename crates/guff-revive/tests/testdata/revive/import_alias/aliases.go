// Package aliases carries one import alias of every shape the
// `import-alias-naming` expressions sort on.
//
// `Configure` takes either a string — the allow expression — or a map with
// `allowRegex` / `denyRegex`, and falls back to `^[a-z][a-z0-9]{0,}$` when
// neither side is set:
//
//	switch namingRule := arguments[0].(type) {
//	case string:         r.setAllowRule(namingRule)
//	case map[string]any: … isRuleOption(k, "allowRegex") / "denyRegex" …
//	}
//	if r.allowRegexp == nil && r.denyRegexp == nil { … default … }
//
// guff had the default baked in and no deny side at all. telegraf configures
// `^[a-z][a-z0-9_]*[a-z0-9]+$` — which allows the underscore its aliases use —
// and that was 128 findings golangci-lint does not make.
//
// `_` and `.` are other rules' business and are never reported here.
package aliases

import (
	"fmt"
	plain "os"
	with_underscore "path/filepath"
	x9 "strconv"
	Upper "strings"
	_ "unsafe"
)

var _ = fmt.Sprint
var _ = plain.Getenv
var _ = with_underscore.Join
var _ = Upper.ToLower
var _ = x9.Itoa
