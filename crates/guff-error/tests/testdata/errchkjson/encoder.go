package encoder

import (
	"encoding/json"
	"io"
)

type safe struct {
	A string
	B int
}

type unsafeT struct {
	F float64
}

// The three call shapes real code uses, none of which guff reported: the
// callee is a *method*, and the rule table spells methods with a receiver.
func exprStmtOnCallResult(w io.Writer) {
	json.NewEncoder(w).Encode(safe{})
}

func blankAssign(w io.Writer) {
	_ = json.NewEncoder(w).Encode(safe{})
}

func exprStmtOnVariable(enc *json.Encoder) {
	enc.Encode(safe{})
}

// Encode forces omit-safe, so an *unsafe* payload is reported even under the
// default settings, and carries the "unsafe type" suffix.
func unsafePayload(w io.Writer) {
	json.NewEncoder(w).Encode(unsafeT{})
}

// Handled errors stay silent, exactly as before.
func returned(w io.Writer) error {
	return json.NewEncoder(w).Encode(safe{})
}

func assignedAndChecked(w io.Writer) {
	if err := json.NewEncoder(w).Encode(safe{}); err != nil {
		_ = err
	}
}
