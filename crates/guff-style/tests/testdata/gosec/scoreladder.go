// Package gosec_scoreladder is the fixture for gosec's `severity` and
// `confidence` keys.
//
// golangci-lint's `filterIssues` keeps an issue when
// `i.Severity >= severity && i.Confidence >= confidence`, and both thresholds
// come from `convertToScore`, where the empty string and "low" are the same
// value. So measuring either key needs findings that differ in *that* score
// while agreeing on the other — one rule per rung:
//
//	rule   severity  confidence
//	G104   low       high
//	G401   medium    high
//	G404   high      medium
//	G101   high      low
//
// Raising `severity` walks up the first column (4 → 3 → 2 findings); raising
// `confidence` walks up the second (4 → 3 → 2), and the two orders disagree,
// which is what makes it a test of two keys rather than one.
package gosec_scoreladder

import (
	"crypto/md5"
	"math/rand"
)

// G101, high severity / low confidence: the name matches gosec's credential
// pattern and the value is long enough to pass its entropy check.
const dataplatformPasswordSecretName = "merpay-dataplatform-jp-alloydb-password"

func mayFail() error { return nil }

// G104, low severity: the returned error is dropped.
func Unhandled() {
	mayFail()
}

// G401, medium severity: md5 is a weak hash.
func WeakHash() {
	_ = md5.New()
}

// G404, high severity / medium confidence: math/rand is not a CSPRNG.
func WeakRandom() int {
	return rand.Int()
}
