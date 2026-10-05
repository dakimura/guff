// want package:`.*sumTypeFact\{T \[A B C\]\}.*`
package no_missing

//sumtype:decl
type T interface{ sealed() }

type A struct{}

func (a *A) sealed() {}

type B struct{}

func (b *B) sealed() {}

type C struct{}

func (c *C) sealed() {}

func main() {
	switch T(nil).(type) {
	case *A, *B, *C:
	}
}
