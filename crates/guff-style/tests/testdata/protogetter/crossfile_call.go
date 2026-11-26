package crossfile

import "example.com/pb"

// A bare call filters an optional argument only when the *parser* resolves the
// name (`fun.Obj.Kind == ast.Fun`), and the parser resolves within one file:
// `takeNick` is declared in crossfile_decl.go, so the argument is reported
// here even though `u.GetNickname()` would not compile. `takeNickLocal` is in
// this file, so the same call shape is filtered.
func passOptionalAcrossFiles(u *pb.User) {
	takeNick(u.Nickname)
	takeNickLocal(u.Nickname)
}

func takeNickLocal(*string) {}
