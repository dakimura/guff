package ok

import "example.com/govet/composites/other"

func ok() {
	_ = other.Config{Err: nil}
}

// A **local alias** is a local type. Upstream's `isLocalType` opens with
// `types.Unalias`, so `basic{...}` here is the same as `BasicAuth{...}` and an
// unkeyed literal of it is allowed. gitea's
// `modules/auth/httpauth/httpauth_test.go` shortens its table that way and had
// six findings because of it.
type BasicAuth struct {
	User string
	Pass string
}

type basic = BasicAuth

func okLocalAlias() BasicAuth {
	return basic{"foo", "bar"}
}

func okLocalAliasPointer() *BasicAuth {
	return &basic{"foo", "bar"}
}
