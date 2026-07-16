package iface

// Identical method sets — both should be reported.
type Pinger interface {
	Ping() error
}

type Healthcheck interface {
	Ping() error
}

// Unused within the package (only reported when enable includes unused).
type Granter interface {
	Grant(permission string) error
}

// Used — should not be reported by unused.
type Allower interface {
	Allow(permission string) error
}

func Allow(x any) {
	_ = x.(Allower)
}
