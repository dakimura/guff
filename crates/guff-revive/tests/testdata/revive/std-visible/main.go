// Since golangci-lint 2.15.0 revive's own type check resolves the standard
// library and nothing else: calls into std are seen, calls into another
// package of the same module are not.
package rv

import (
	"bytes"
	"errors"
	"fmt"
	"os"

	"example.com/rv/sub"
)

func local() error { return nil }

func Run() {
	fmt.Printf("x")
	errors.New("x")
	os.Remove("x")
	sub.E()
	local()
	var b bytes.Buffer
	b.WriteString("x")
}

type stdBuf = bytes.Buffer

type subRes = sub.Res

type own struct{}

type ownAlias = own

func StdAlias() *stdBuf { return nil }

func SubAlias() *subRes { return nil }

func OwnAlias() *ownAlias { return nil }
