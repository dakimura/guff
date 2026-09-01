package depr

import "example.com/sa1019groupdoc/dep"

var (
	_ = dep.GroupA
	_ = dep.GroupB
	_ = dep.MixA
	_ = dep.MixB
	_ = dep.MixC
	_ = dep.PairA
	_ = dep.PairB
	_ = dep.PairC
	_ dep.TypeA
	_ dep.TypeB
	_ = dep.LineA
	_ = dep.LineB
	_ = dep.ParaA
	_ = dep.ParaB
)

func Use2(f dep.Fields, i dep.Iface) {
	_ = f.Plain
	_ = f.Old
	_ = f.Also
	i.Plain()
	i.Old()
	i.Also()
}
