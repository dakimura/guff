// Not upstream: the shapes revive v1.17.1's checkFunc walks that its own
// testdata does not, measured against golangci-lint 2.15.0.
package shapes

func use(...any) {}

func mk(int) func(int) int { return nil }

type box struct{}

func (box) M() {}

var arr [20]box

var fns []func(int)

func shapes(x int) {
	go func() { x = 1 }()              // a FuncLit argument-less call: Fun is walked
	use(x + 2)                         // a binary expression argument
	fns[3](4)                          // a literal in an index Fun
	_ = mk(5)(6)                       // a call as Fun
	arr[13].M()                        // a literal in a selector's operand
	defer use(10)                      // defer
	go use(11)                         // go
	_ = int64(12)                      // a conversion is a CallExpr too
	use(func() { const k = 7; _ = k }) // a declaration inside a FuncLit argument
	use(func() {
		use(struct {
			F int `json:"inner"`
		}{F: 14})
	})
	h := func() int { return 8 } // a FuncLit outside any call
	_ = h
	_ = []int{9}
	use('a', 2i, 1.5) // CHAR and IMAG are never reported
	use("perfile")
	use("perfile")
}
