package draw

import "image"

// `draw.Draw($x, $_, $x, $_, $_)` is the one dupArg pattern that compares
// arguments 0 and 2.

type Op int

const Src Op = 0

type Image interface {
	image.Image
	Set(x, y int, c interface{})
}

func Draw(dst Image, r image.Rectangle, src image.Image, sp image.Point, op Op) {}
