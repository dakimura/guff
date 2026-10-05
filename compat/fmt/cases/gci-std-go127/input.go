package p

import (
	"crypto/hpke"
	"crypto/mldsa"
	"crypto/mlkem/mlkemtest"
	"example.com/other"
	"fmt"
	"runtime/secret"
	"testing/cryptotest"
	"uuid"
)

var _ = []any{hpke.X, mldsa.X, mlkemtest.X, other.X, fmt.Sprint, secret.X, cryptotest.X, uuid.X}
