package exhaustive

type Direction int

const (
	North Direction = iota
	East
	South
	West
)

var incomplete = map[Direction]string{ // want missing South, West
	North: "north",
	East:  "east",
}

var complete = map[Direction]string{
	North: "north",
	East:  "east",
	South: "south",
	West:  "west",
}

// Empty map literals are deliberately ignored by exhaustive.
var empty = map[Direction]string{}
