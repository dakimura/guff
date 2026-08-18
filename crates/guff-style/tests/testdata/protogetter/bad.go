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
