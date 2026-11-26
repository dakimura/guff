package image

// Enough of `image` for the dupArg fixture's `draw.Draw` call: the fixture only
// passes interface values around, it never implements one.

type Point struct{ X, Y int }

type Rectangle struct{ Min, Max Point }

type Image interface {
	Bounds() Rectangle
}
