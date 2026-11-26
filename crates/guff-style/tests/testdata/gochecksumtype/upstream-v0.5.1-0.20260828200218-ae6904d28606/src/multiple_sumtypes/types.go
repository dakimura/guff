// want package:`.*sumTypeFact\{First \[FirstA FirstB\]; Second \[SecondA SecondB\]\}.*`
package multiple_sumtypes

//sumtype:decl
type First interface{ first() }

type FirstA struct{}

func (*FirstA) first() {}

type FirstB struct{}

func (*FirstB) first() {}

//sumtype:decl
type Second interface{ second() }

type SecondA struct{}

func (*SecondA) second() {}

type SecondB struct{}

func (*SecondB) second() {}
