package exhaustive

type Color int

const (
	Red Color = iota
	Green
	Blue
)

func withDefault(c Color) {
	switch c {
	case Red:
	default:
	}
}
