package math

// Pi is here for the `math.Pow(math.Pi, 2)` shape: a SelectorExpr naming an
// untyped-float constant, which is the one spelling of "no conversion needed"
// that does not go through an identifier in the file under test.
const Pi = 3.141592653589793

func Pow(x, y float64) float64 { return 0 }
