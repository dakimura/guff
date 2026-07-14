package errname

type DNSConfigError struct{}

func (DNSConfigError) Error() string { return "dns" }

var (
	ErrEndOfFile error
	errEndOfFile error
)
