module example.com/parens

// go 1.21, not the current release: revive's range-val-address returns early
// for go1.22+, where each iteration gets its own copy of the range variable.
// Below 1.22 the rule applies and the parenthesis shape can be compared at all.
go 1.21
