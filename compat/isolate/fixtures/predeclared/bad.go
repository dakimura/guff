package p

// predeclared reports each kind of declaration that shadows a builtin, and the
// message names the kind — so a func and a variable are separate arms, not the
// same one twice.

func len(x int) int { return x }

type copy struct{}

const cap = 1

var recover = 1

func Bad() {
	error := "oops"
	_ = error

	var new int
	_ = new

	for append := range 3 {
		_ = append
	}
}

func WithParams(min int, max int) int { return min + max }

func (copy) delete() {}
