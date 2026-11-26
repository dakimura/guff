// want package:`.*sumTypeFact\{T1 \[A B C T2\]\}.*`
package all_leaves

//sumtype:decl
type T1 interface{ sealed1() }

type T2 interface {
	T1
	sealed2()
}

type A struct{}

func (a *A) sealed1() {}

type B struct{}

func (b *B) sealed1() {}
func (b *B) sealed2() {}

type C struct{}

func (c *C) sealed1() {}
func (c *C) sealed2() {}

func main() {
	switch T1(nil).(type) {
	case *A:
	case *B:
	case *C:
	}
}
