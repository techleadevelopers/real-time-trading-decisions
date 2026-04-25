package bingx

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"net/url"
	"sort"
)

func sign(secret string, params map[string]string) string {
	query := canonicalQuery(params)
	mac := hmac.New(sha256.New, []byte(secret))
	_, _ = mac.Write([]byte(query))
	return hex.EncodeToString(mac.Sum(nil))
}

func canonicalQuery(params map[string]string) string {
	keys := make([]string, 0, len(params))
	for key := range params {
		if key == "signature" {
			continue
		}
		keys = append(keys, key)
	}
	sort.Strings(keys)
	values := url.Values{}
	for _, key := range keys {
		values.Set(key, params[key])
	}
	return values.Encode()
}
