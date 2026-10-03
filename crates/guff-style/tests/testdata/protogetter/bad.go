package p

import "example.com/pb"

func useName(u *pb.User) string {
	return u.Name
}

func useAge(u *pb.User) int32 {
	return u.Age
}

func useChain(u *pb.User) string {
	return u.Address.City
}

// A getter that *does* return a pointer is left alone by the nil-comparison
// filter and reported by the ordinary selector rule. The asymmetry is
// upstream's.
func nilComparePointerGetter(u *pb.User) bool {
	return u.Address == nil
}

// Upstream filters the *left* operand's position, so the reversed spelling
// filters the `nil` and the field is still reported.
func nilCompareReversed(u *pb.User) bool {
	return nil == u.Meta
}

// The pointer test is on the *target*, not on the field: a non-pointer target
// keeps the finding even when the value is an optional field, because
// `u.GetNickname()` fits there.
func optionalIntoStringField(u *pb.User) *pb.Address {
	return &pb.Address{City: u.Name}
}

// A func-typed variable is not an `ast.Fun`, so the optional field passed to it
// is reported even though the getter would not compile there.
func passOptionalToFuncValue(u *pb.User) {
	f := func(*string) {}
	f(u.Nickname)
}

// Only optional fields are filtered: a plain field passed to a method or to a
// function of this file is reported as usual.
type nameSink struct{}

func (nameSink) takeName(string) {}

func takeName(string) {}

func passPlainField(s nameSink, u *pb.User) {
	s.takeName(u.Name)
	takeName(u.Name)
}

// A declaration without a pointer type keeps the finding.
func declareValue(u *pb.User) string {
	var n = u.Name
	return n
}
