// want package:`.*sumTypeFact\{T \[A B\]\}.*`
package with_alias

//sumtype:decl
type T interface{ sealed() }

type A struct{}

func (*A) sealed() {}

type Alias = A

type B struct{}

func (*B) sealed() {}

func main() {
	switch T(nil).(type) { // want `missing cases for A$`
	case *B:
	}
}
