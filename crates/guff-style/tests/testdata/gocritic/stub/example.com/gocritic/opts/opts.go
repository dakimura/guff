// Package opts is the "another package" half of dupOption's variadic option
// call: cluster-api's `util/collections` declares `func And(filters ...Func)`
// over a *named* func type, which is what guff failed to recognise.
package opts

type Filter func(int) bool

func And(filters ...Filter) Filter { return filters[0] }

// Variadic over a non-func element: not an option type.
func Sum(ns ...int) int { return len(ns) }

type Thunk func()

// Variadic over a func with no parameters: not an option type either.
func Run(ts ...Thunk) {}
