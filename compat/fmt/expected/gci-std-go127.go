package p

import (
	"crypto/hpke"
	"crypto/mldsa"
	"crypto/mlkem/mlkemtest"
	"fmt"
	"runtime/secret"
	"testing/cryptotest"
	"uuid"

	"example.com/other"
)

var _ = []any{hpke.X, mldsa.X, mlkemtest.X, other.X, fmt.Sprint, secret.X, cryptotest.X, uuid.X}
