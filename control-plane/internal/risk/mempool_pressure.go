package risk

import "math"

type MempoolPressureModel struct {
	burstEMA          float64
	clusterEMA        float64
	failEMA           float64
	outbidEMA         float64
	competitionEMA    float64
	latencyPenaltyEMA float64
	lastSymbolHash    uint32
	initialized       bool
}

func (m *MempoolPressureModel) ObserveMarket(symbol string, tradeVolume, tradeVelocity float64) float64 {
	hash := symbolHash(symbol)
	cluster := 0.0
	if m.initialized && hash == m.lastSymbolHash {
		cluster = 1.0
	}
	m.lastSymbolHash = hash
	m.initialized = true
	burst := clamp01(math.Abs(tradeVelocity)/80.0 + tradeVolume/25.0)
	m.burstEMA = ema(m.burstEMA, burst, 0.08)
	m.clusterEMA = ema(m.clusterEMA, cluster, 0.04)
	m.competitionEMA = ema(m.competitionEMA, clamp01(0.60*burst+0.40*m.clusterEMA), 0.06)
	return m.Score()
}

func (m *MempoolPressureModel) ObserveExecutionFailure() float64 {
	m.failEMA = ema(m.failEMA, 1.0, 0.12)
	m.outbidEMA = ema(m.outbidEMA, 1.0, 0.10)
	m.competitionEMA = ema(m.competitionEMA, 1.0, 0.08)
	return m.Score()
}

func (m *MempoolPressureModel) ObserveExecutionSuccess() float64 {
	m.failEMA = ema(m.failEMA, 0.0, 0.04)
	m.outbidEMA = ema(m.outbidEMA, 0.0, 0.03)
	m.latencyPenaltyEMA = ema(m.latencyPenaltyEMA, 0.0, 0.03)
	return m.Score()
}

func (m *MempoolPressureModel) ObserveLatencyPenalty(penalty float64) {
	m.latencyPenaltyEMA = ema(m.latencyPenaltyEMA, clamp01(penalty), 0.08)
}

func (m *MempoolPressureModel) Score() float64 {
	return clamp01(0.34*m.burstEMA + 0.20*m.clusterEMA + 0.20*m.failEMA + 0.14*m.outbidEMA + 0.12*m.latencyPenaltyEMA)
}

func (m *MempoolPressureModel) CompetitionIntensityScore() float64 {
	return clamp01(0.42*m.competitionEMA + 0.28*m.clusterEMA + 0.20*m.burstEMA + 0.10*m.failEMA)
}

func (m *MempoolPressureModel) LatencyAdvantagePenalty() float64 {
	return clamp01(m.latencyPenaltyEMA)
}

func (m *MempoolPressureModel) OutbidLikelihoodIndex() float64 {
	return clamp01(0.45*m.outbidEMA + 0.30*m.burstEMA + 0.25*m.failEMA)
}

func symbolHash(symbol string) uint32 {
	var h uint32 = 2166136261
	for i := 0; i < len(symbol); i++ {
		h ^= uint32(symbol[i])
		h *= 16777619
	}
	return h
}
