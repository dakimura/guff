package p

import "pb"

func good(u *pb.User) string {
	name := u.GetName() // already using the getter
	u.Name = "x"        // assignment LHS is a write, skip
	pAge := &u.Age      // address-of, skip
	_ = pAge
	return name
}

type Local struct {
	Field string
}

func localOk(l *Local) string {
	// Not a proto message (no ProtoReflect / ProtoMessage), so leave it.
	return l.Field
}
