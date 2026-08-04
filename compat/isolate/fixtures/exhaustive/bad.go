package p

type Color int

const (
	Red Color = iota
	Green
	Blue
)

func Bad(c Color) string {
	switch c {
	case Red:
		return "r"
	case Green:
		return "g"
	}
	return ""
}
