package p

// iface defaults to the `identical` analyzer alone (golangci's
// `analyzersFromSettings`), so an interface has to have a twin to be reported.
// The previous fixture had one interface with an unexported method, which no
// enabled analyzer looks at.
type Reader interface {
	Read() error
}

type Fetcher interface {
	Read() error
}
