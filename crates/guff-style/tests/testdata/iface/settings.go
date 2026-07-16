package ifacesettings

type Alpha interface {
	Do() error
}

type Beta interface {
	Do() error
}

type Orphan interface {
	Orphan()
}
