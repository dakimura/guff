// want package:`.*sumTypeFact\{T \[A B\]\}.*`
package missing_one

//sumtype:decl
type T interface{ sealed() }

type A struct{}

func (a *A) sealed() {}

type B struct{}

func (b *B) sealed() {}

func main() {
	switch T(nil).(type) { // want `exhaustiveness check failed for sum type "T" \(from .*main.go:5:6\): missing cases for B`
	case *A:
	}
}
