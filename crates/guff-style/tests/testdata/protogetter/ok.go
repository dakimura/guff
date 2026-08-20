package p

import "example.com/pb"

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

// `msg.Field == nil` where `GetField` returns something that is not a pointer
// is filtered upstream: the getter answers the question identically, so there
// is nothing to rewrite. dapr writes 80 of these (`if req.Metadata == nil`).
func nilCompareNonPointerGetter(u *pb.User) bool {
	return u.Meta == nil
}

func nilCompareNonPointerGetterNeq(u *pb.User) bool {
	return u.Meta != nil
}

// `append(msg.Field, …)` keeps the direct read: rewriting the first argument to
// a getter changes what append may write in place, so upstream filters it
// (`replace-first-arg-in-append` is off by default).
func appendToField(u *pb.User, more string) {
	u.Names = append(u.Names, more)
}

// A message reached through a *type alias* is not a proto message to
// protogetter at all: its `typesNamed` asserts `t.(*types.Named)` with no
// `Unalias`, and since Go 1.23 an aliased type is a `*types.Alias`. dapr
// reaches every durabletask message this way, which is why golangci-lint
// reports nothing for the whole repo.
type aliasUser = pb.User

func viaAlias(u *aliasUser) string {
	return u.Name
}

// `Nickname: u.Nickname` in a keyed literal: the field is `*string` and
// `GetNickname()` is `string`, so the rewrite would not compile and upstream
// filters the position (`hasPointerKeyWithoutPointerGetter`). dapr fills
// sixteen optional proto fields this way, in `pkg/api/universal/jobs.go` above
// all.
func copyOptionalIntoLiteral(u *pb.User) *pb.User {
	return &pb.User{
		Nickname: u.Nickname,
		Name:     u.GetName(),
	}
}

// Same test on the left-hand side of an assignment.
func copyOptionalIntoPointer(u *pb.User, dst *string) {
	dst = u.Nickname
	_ = dst
}
