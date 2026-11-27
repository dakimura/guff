//go:build go1.21

package extra

type ptrOnly int64

func (*ptrOnly) String() string { return "" }

func _(p ptrOnly) {
	println(string(p)) // pointer-receiver methods only: %c + %d conflict
}
