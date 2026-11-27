//go:build go1.21

package extra

type text string

type rn = int64

func _(r rn) {
	println(text(r)) // alias source, named target: %c + %d conflict
}
