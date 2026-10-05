// revive v1.17.0 dropped `exportedType`'s exception for unexported interface
// types: returning one is reported like any other unexported type, whether it
// is a defined type or an alias. Upstream's own testdata has no such shape.
package urlocal

type iface interface{ M() }

type ifaceAlias = interface{ M() }

type impl struct{}

func (impl) M() {}

func NewIface() iface { return impl{} }

func NewIfaceAlias() ifaceAlias { return impl{} }

// Exported interfaces are fine either way.
type Iface interface{ M() }

func NewExported() Iface { return impl{} }
