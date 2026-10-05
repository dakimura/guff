// want package:`.*sumTypeFact\{T \[A B C\]\}.*`
package missing_two

//sumtype:decl
type T interface{ sealed() }

type A struct{}

func (a *A) sealed() {}

type B struct{}

func (b *B) sealed() {}

type C struct{}

func (c *C) sealed() {}

func main() {
	switch T(nil).(type) { // want `exhaustiveness check failed for sum type "T" .*missing cases for B, C`
	case *A:
	}
}
