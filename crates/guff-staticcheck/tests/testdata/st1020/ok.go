package pkg

// Foo does things.
func Foo() {}

type T struct{}

// Bar does things.
func (T) Bar() {}

// Deprecated: don't use.
func Baz() {}

type u struct{}

// Whatever is fine — receiver type is unexported.
func (u) Whatever() {}
