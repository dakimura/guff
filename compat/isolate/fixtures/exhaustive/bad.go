package p

type Color int

const (
	Red Color = iota
	Green
	Blue
)

// exhaustive reports two shapes with two different messages: an incomplete
// switch and, under `check: [map]`, an incomplete map literal keyed by the
// enum.

func Switch(c Color) string {
	switch c {
	case Red:
		return "r"
	case Green:
		return "g"
	}
	return ""
}

var names = map[Color]string{
	Red:   "r",
	Green: "g",
}

// The message lists every missing member, so a switch missing two is a
// different sentence from one missing one.
type Size int

const (
	Small Size = iota
	Medium
	Large
)

func MissingTwo(s Size) string {
	switch s {
	case Small:
		return "s"
	}
	return ""
}
