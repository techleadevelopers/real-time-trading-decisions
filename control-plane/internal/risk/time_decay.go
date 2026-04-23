package risk

import (
	"math"
	"time"
)

const defaultDecayTauMs = 180.0

func DecayFactor(latency time.Duration, tauMs float64) float64 {
	if tauMs <= 0 {
		tauMs = defaultDecayTauMs
	}
	ms := float64(latency.Microseconds()) / 1000.0
	if ms < 0 {
		ms = 0
	}
	return math.Exp(-ms / tauMs)
}
