// Package ruleexclude is revive's per-rule `exclude` list.
//
// A rule entry may carry `exclude`, and `lint/file.go` skips that rule for any
// file the list matches — before the rule runs:
//
//	ruleConfig := rulesConfig[currentRule.Name()]
//	if ruleConfig.MustExclude(f.Name) { continue }
//
// The patterns are matched against the file name golangci-lint hands revive,
// which is the **absolute** path from the pass, so real configs spell them
// `**/…`. telegraf excludes twenty-three paths from `exported` alone; without
// this, guff reported 2748 findings golangci-lint does not.
//
// This file is kept: `exported` reports its undocumented type.
package ruleexclude

type Kept struct{ N int }
