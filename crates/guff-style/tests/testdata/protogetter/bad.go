package p

import "pb"

func useName(u *pb.User) string {
	return u.Name
}

func useAge(u *pb.User) int32 {
	return u.Age
}

func useChain(u *pb.User) string {
	return u.Address.City
}
