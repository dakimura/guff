package gocritic

// ptrToRefParam's `isRefType` switches on the pointer's element as written and
// has no `*types.Alias` arm, and its `TypeOf(param.Type).(*types.Pointer)` does
// not look through one either. go/types materializes aliases, so everything
// spelled through an alias — `any` included — is silent. trivy's
// `unmarshalIntFirst(dec *jsontext.Decoder, v *any)` is the first shape.
// Measured against golangci-lint 2.12.2 (go-critic v0.14.3).

type p2rMapAlias = map[string]int
type p2rIfaceAlias = interface{ M() }
type p2rNamedMap map[string]int
type p2rNamedIface interface{ M() }
type p2rAliasOfNamed = p2rNamedIface
type p2rPtrMapAlias = *map[string]int

func p2rAny(v *any)                     {} // silent: `any` is an alias
func p2rEmptyIface(v *interface{})      {} // reported
func p2rError(v *error)                 {} // reported: a Named interface
func p2rMap(v *map[string]int)          {} // reported
func p2rMapAliasP(v *p2rMapAlias)       {} // silent
func p2rIfaceAliasP(v *p2rIfaceAlias)   {} // silent
func p2rNamedMapP(v *p2rNamedMap)       {} // silent: only a Named *interface* counts
func p2rNamedIfaceP(v *p2rNamedIface)   {} // reported
func p2rAliasOfNamedP(v *p2rAliasOfNamed) {} // silent: the alias hides the Named
func p2rChan(v *chan int)               {} // reported
func p2rResultAny() (v *any)            { return nil } // silent
func p2rPtrAlias(v p2rPtrMapAlias)      {} // silent: not a *types.Pointer
