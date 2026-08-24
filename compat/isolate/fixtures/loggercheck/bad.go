package p

import "log/slog"

// loggercheck knows four logger families and says the same kind of thing about
// each; the message names what it found, so odd counts and non-string keys are
// separate arms.

func OddKeyvals() {
	slog.Info("msg", "only_key")
}

func NonStringKey() {
	slog.Info("msg", 42, "value")
}

func OddOnWith() {
	_ = slog.With("k")
}

// no-printf-like
func PrintfLike(name string) {
	slog.Info("hello %s", name)
}
