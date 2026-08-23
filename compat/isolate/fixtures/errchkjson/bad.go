package p

import (
	"encoding/json"
	"io"
)

func Bad() {
	var f float64
	_, _ = json.Marshal(f) // unsafe type, error discarded
}

// `(*encoding/json.Encoder).Encode` is a *method*, and upstream's rule table
// keys off `types.Func.FullName()`, which spells methods with a receiver. A
// port that resolves the callee to package-path-plus-name reports none of
// these while the `Marshal` line above keeps working — which is how this
// fixture passed at one finding for as long as it did.
func BadEncoder(w io.Writer) {
	json.NewEncoder(w).Encode("x")     // expression statement
	_ = json.NewEncoder(w).Encode("y") // assigned to blank
}

func BadEncoderVar(enc *json.Encoder) {
	enc.Encode("z") // receiver is a plain variable, not a call result
}

func BadEncoderUnsafe(w io.Writer) {
	var f float64
	json.NewEncoder(w).Encode(f) // Encode forces omit-safe: unsafe type reported
}

// Handled errors must stay silent on both sides.
func OKReturned(w io.Writer) error {
	return json.NewEncoder(w).Encode("ok")
}

func OKChecked(w io.Writer) {
	if err := json.NewEncoder(w).Encode("ok"); err != nil {
		_ = err
	}
}
