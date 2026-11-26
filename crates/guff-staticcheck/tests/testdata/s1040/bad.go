package main

import "example.com/s1040/inner"

type msg interface{ M() }

// alias is to msg what proto.Message is to protoreflect.ProtoMessage.
type alias = msg

type box struct{}

func (box) get() msg { return nil }

func mk() box { return box{} }

func f(i interface{}) { _ = i.(interface{}) }

func single(x msg) msg { return x.(msg) }

func commaOk(x msg) (msg, bool) {
	v, ok := x.(msg)
	return v, ok
}

// The alias. `types.Identical` sees through it; comparing rendered type strings
// does not, which is the whole of boundary's miss.
func throughAnAlias(x msg) (alias, bool) {
	v, ok := x.(alias)
	return v, ok
}

// An imported interface, for the message text.
func imported(x inner.Msg) (inner.Msg, bool) {
	v, ok := x.(inner.Msg)
	return v, ok
}

// The operand is a call chain, as boundary writes it.
func callChain() (msg, bool) {
	v, ok := mk().get().(msg)
	return v, ok
}
