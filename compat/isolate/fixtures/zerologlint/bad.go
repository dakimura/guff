package p

import "github.com/rs/zerolog/log"

func Bad() {
	log.Info().Str("k", "v")
}

// zerologlint reports each event chain that is never dispatched, so a second
// one is a second finding — and `Msg` / `Send` are the negatives.
func AlsoBad() {
	log.Error().Int("n", 1)
}

func Dispatched() {
	log.Info().Str("k", "v").Msg("done")
}
