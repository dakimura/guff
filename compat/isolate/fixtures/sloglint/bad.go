package p

import "log/slog"

// sloglint enables `no-mixed-args` by default and nothing else, so the fixture
// has to mix loose key-value pairs with an slog.Attr in one call.
func Bad() {
	slog.Info("msg", "k", 1, slog.Int("n", 2))
}
