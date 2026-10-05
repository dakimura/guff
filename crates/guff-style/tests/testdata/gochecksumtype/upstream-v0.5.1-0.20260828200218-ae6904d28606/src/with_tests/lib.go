// want package:`.*sumTypeFact\{T \[A B\]\}.*`
package with_tests

//sumtype:decl
type T interface{ sealed() }

type A struct{}

func (a *A) sealed() {}

type B struct{}

func (b *B) sealed() {}
