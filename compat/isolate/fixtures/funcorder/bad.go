package p

// funcorder says six different things, each naming the two declarations whose
// order it objects to. One struct with two methods reaches one of them.

// "constructor %q for struct %q should be placed after the struct declaration"
func NewT() *T { return &T{} }

type T struct{}

func (T) Exported() {}

// "unexported method %q for struct %q should be placed after the exported
// method %q"
func (T) unexported() {}

func (T) AlsoExported() {}

type U struct{}

func (U) Method() {}

// "constructor %q for struct %q should be placed before struct method %q"
func NewU() *U { return &U{} }

type V struct{}

// "constructor %q for struct %q should be placed before constructor %q"
func NewVFromString(string) *V { return &V{} }

func NewV() *V { return &V{} }

type W struct{}

// "method %q for struct %q should be placed before method %q" — alphabetical.
func (W) Zeta() {}

func (W) Alpha() {}

// The sixth message ("unexported function %q should be placed after the
// exported function %q") needs `function`, which golangci-lint 2.12.2 has no
// config key for. This is the shape that would trigger it; upstream is silent,
// so it is a negative case until the pin moves.
func Exported() {}

func helper() {}

func AlsoExportedFunc() {}
