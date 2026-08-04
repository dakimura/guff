package p

import "github.com/rs/zerolog/log"

func Bad() {
	log.Info().Str("k", "v")
}
