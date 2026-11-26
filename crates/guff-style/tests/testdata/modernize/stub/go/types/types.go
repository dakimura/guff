package types

// Minimal stub of go/types used by the stditerators fixture. Only the
// legacy Len/At-style methods needed for detection are defined.

type Var struct{}

func (v *Var) Name() string { return "" }

type Struct struct{}

func (s *Struct) NumFields() int      { return 0 }
func (s *Struct) Field(i int) *Var    { return nil }

type Tuple struct{}

func (t *Tuple) Len() int          { return 0 }
func (t *Tuple) At(i int) *Var     { return nil }
