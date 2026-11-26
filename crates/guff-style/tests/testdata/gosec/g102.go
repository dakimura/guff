// Package g102 is gosec's G102: a listener bound to every interface.
//
// The address does not have to be written at the call. `GetIdentStringValues`
// follows an identifier to its declaration and reads the string literals there
// — one hop, and only a literal, so a concatenation resolves to nothing. The
// resolution is the parser's (`ident.Obj.Decl`), so it reaches a declaration in
// the same file and no further.
//
// syncthing `cmd/infra/strelaypoolsrv` is the shape: `listen = ":80"` at the
// top of the file, handed to both `tls.Listen` and `net.Listen`.
package g102

import (
	"crypto/tls"
	"net"
)

var (
	listen   = ":80"
	loopback = "127.0.0.1:80"
	allIPv4  = "0.0.0.0:80"
	assigned string
	fromCall = pick()
)

func pick() string { return ":80" }

// fires — a string literal.
func Literal() (net.Listener, error) { return net.Listen("tcp", ":8080") }

// silent — a loopback literal.
func LiteralLoopback() (net.Listener, error) { return net.Listen("tcp", "127.0.0.1:8080") }

// fires — a package-level var initialized with a matching literal.
func VarListen() (net.Listener, error) { return net.Listen("tcp", listen) }

// fires — `0.0.0.0` through a var.
func VarAll() (net.Listener, error) { return net.Listen("tcp", allIPv4) }

// silent — a var holding a loopback address.
func VarLoopback() (net.Listener, error) { return net.Listen("tcp", loopback) }

// silent — a var with no initializer: there is no string to read.
func VarNoValue() (net.Listener, error) { return net.Listen("tcp", assigned) }

// silent — a var initialized from a call, which `GetString` does not accept.
func VarFromCall() (net.Listener, error) { return net.Listen("tcp", fromCall) }

// fires — a local assigned a matching literal.
func LocalAssign() (net.Listener, error) {
	addr := ":9000"

	return net.Listen("tcp", addr)
}

// fires — `crypto/tls.Listen` is on the call list beside `net.Listen`.
func TLSListen(cfg *tls.Config) (net.Listener, error) { return tls.Listen("tcp", listen, cfg) }

// silent — a parameter: its declaration is not a node the resolution reads.
func ParamAddr(addr string) (net.Listener, error) { return net.Listen("tcp", addr) }

// silent — `net.Dial` is not on the call list.
func Dial() (net.Conn, error) { return net.Dial("tcp", ":80") }
