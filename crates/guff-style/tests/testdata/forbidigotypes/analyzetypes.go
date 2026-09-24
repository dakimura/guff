package forbidigotypes

import (
	"fmt"
	osu "os/user"
	user2 "os/user"
	"strings"
)

// With `analyze-types`, the text a pattern is matched against is the resolved
// one — the imported package's *name*, never the local alias, and a type's
// name rather than the variable's. Every shape was measured against
// golangci-lint 2.12.2 (teleport forbids `os/user` this way).

// An aliased import of a forbidden package.
func AliasedCall(n string) (*osu.User, error) { return osu.Lookup(n) }

// A second alias of the same package, so "the alias happens to be the package
// name" cannot be what makes it match.
func SecondAlias(n string) (*user2.User, error) { return user2.LookupId(n) }

// A method on a parameter whose type comes from the package: matched as
// `user.User.GroupIds`.
func MethodOnParam(u *osu.User) ([]string, error) { return u.GroupIds() }

// The same method on a local variable.
func MethodOnLocal() ([]string, error) {
	u, err := osu.Current()
	if err != nil {
		return nil, err
	}
	return u.GroupIds()
}

// A plain, unaliased import — the shape that already worked.
func PlainImport() { fmt.Println("x") }

// An unrelated selector.
func Unrelated(s string) string { return strings.ToUpper(s) }

// A local type named `user` with a method named `Lookup`: the *source text*
// `v.Lookup` matches the pattern and the resolved text
// `forbidigotypes.user.Lookup` does not, so nothing is reported.
type user struct{}

func (user) Lookup(string) (*osu.User, error) { return nil, nil }

func LocalTypeNamedUser() {
	var v user
	_, _ = v.Lookup("x")
}
