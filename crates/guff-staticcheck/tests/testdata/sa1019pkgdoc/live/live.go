package live

// A package with no deprecation at all: importing it must stay silent, and
// every one of its files is visited before the scan gives up. That is the
// case whose cost the filename restriction was protecting.
func Fine() int { return 3 }
