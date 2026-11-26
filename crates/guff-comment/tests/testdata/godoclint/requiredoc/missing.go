package requiredoc

// Every symbol below is missing a godoc. The //foo:bar makes the trailing
// comment a *directive*, which `ast.CommentGroup.Text()` drops — so the
// trailing group exists but its text is empty, and the symbol is still
// undocumented. Without it these lines would be satisfied by their own
// "// want" annotation.

const SingleSingleFooNG = 0 //foo:bar

const SingleMultiFooNG, SingleMultiBarNG = 0, 0 //foo:bar

const (
	MultiSingleFooNG = 0 //foo:bar
)

const (
	MultiMultiFooNG, MultiMultiBarNG = 0, 0 //foo:bar
)

type SingleTFooNG int //foo:bar

type (
	MultiTFooNG int //foo:bar
)

func FooNG() {} //foo:bar

type TFooNG string //foo:bar

func (*TFooNG) TFooBarNG() {} //foo:bar

func (*TFooNG) tFooBarNG() {} //foo:bar

const singleSingleFooNG = 0 //foo:bar

const (
	multiSingleFooNG = 0 //foo:bar
)

type singleTFooNG int //foo:bar

func funcFooNG() {} //foo:bar

type tFooNG string //foo:bar

func (*tFooNG) TFooBarNG() {} //foo:bar
