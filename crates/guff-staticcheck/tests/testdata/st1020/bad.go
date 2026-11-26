package pkg

// whatever
func foo() {}

// Foo is amazing
func Foo() {}

// Whatever
func Bar() {}

type T struct{}

// Whatever
func (T) foo() {}

// Foo is amazing
func (T) Foo() {}

// Whatever
func (T) Bar() {}

// Deprecated: don't use.
func (T) Dep() {}

//
func Qux() {}

// Meow is amazing.
func Meow() {}

//some:directive
func F1() {}

//some:directive
// F2 is amazing
func F2() {}

//some:directive
// Whatever
func F3() {}

// Deprecated: don't use.
func F4() {}

// wrong comment yo.
//
// Deprecated: don't use.
func F6() {}
