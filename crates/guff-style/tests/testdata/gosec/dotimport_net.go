// Package dotimportnet is G102 (bind to all interfaces) through a dot import.
//
// `net` gets a file of its own because `net.Pipe` and `io.Pipe` cannot be
// dot-imported into the same file — see dotimport_io.go for the other half.
package dotimportnet

import (
	"net"
	. "net"
)

// silent — `bind`'s call list is `ContainsPkgCallExpr`, and a dot-imported
// `Listen` names no package.
func DotListen() (Listener, error) { return Listen("tcp", ":8080") }

// fires — package-qualified, bind-all address.
func QualifiedListen() (net.Listener, error) { return net.Listen("tcp", ":8080") }
