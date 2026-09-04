package main

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

type cardUpdateRoundTripFunc func(*http.Request) (*http.Response, error)

func (fn cardUpdateRoundTripFunc) RoundTrip(request *http.Request) (*http.Response, error) {
	return fn(request)
}

func TestCompareCardVersion(t *testing.T) {
	cases := []struct {
		left, right string
		want        int
	}{
		{"4.0.4", "v4.0.4", 0},
		{"4.0.4", "v4.2.0", -1},
		{"4.0.4", "v4.0.0", 1},
		{"5.0.0", "v4.9.9", 1},
	}
	for _, tc := range cases {
		got, err := compareCardVersion(tc.left, tc.right)
		if err != nil || got != tc.want {
			t.Fatalf("compareCardVersion(%q, %q) = %d, %v; want %d", tc.left, tc.right, got, err, tc.want)
		}
	}
	if _, err := compareCardVersion("latest", "v4.0.4"); err == nil {
		t.Fatal("non-semver tag must be rejected")
	}
}

func TestSelectCardUpdateAssetRequiresCanonicalName(t *testing.T) {
	release := cardGitHubRelease{
		TagName: "v4.0.4",
		Assets: []cardGitHubReleaseAsset{
			{Name: "TCM-v4.0.4-MacOS-AArch64-APP.zip", BrowserDownloadURL: "wrong-platform"},
			{Name: "TCM-v4.0.4-Windows-x64-EXE.zip", BrowserDownloadURL: "package"},
		},
	}
	asset, err := selectCardUpdateAsset(release)
	if err != nil {
		t.Fatal(err)
	}
	if asset.BrowserDownloadURL != "package" {
		t.Fatalf("selected wrong asset: %#v", asset)
	}

	release.Assets = []cardGitHubReleaseAsset{{Name: "Tech-Card-Manager-Windows-x64-v4.0.4.zip"}}
	if _, err := selectCardUpdateAsset(release); err == nil {
		t.Fatal("legacy or ambiguous archive names must not satisfy the update selector")
	}
}

func cardUpdateTestRelease(tag string) cardGitHubRelease {
	archive, _ := cardUpdateArchiveName(tag)
	return cardGitHubRelease{
		TagName: tag,
		HTMLURL: "https://github.com/Eric-Hou1997/Tech-Card-Manager/releases/tag/" + tag,
		Assets: []cardGitHubReleaseAsset{{
			Name:               archive,
			BrowserDownloadURL: "https://github.com/Eric-Hou1997/Tech-Card-Manager/releases/download/" + tag + "/" + archive,
		}},
	}
}

func TestClassifyCardUpdate403Kinds(t *testing.T) {
	now := time.Date(2026, 9, 4, 12, 0, 0, 0, time.UTC)
	response := func(headers map[string]string, body string) *http.Response {
		header := make(http.Header)
		for key, value := range headers {
			header.Set(key, value)
		}
		return &http.Response{StatusCode: http.StatusForbidden, Header: header, Body: io.NopCloser(strings.NewReader(body))}
	}
	cases := []struct {
		name    string
		headers map[string]string
		body    string
		want    string
	}{
		{name: "primary", headers: map[string]string{"X-RateLimit-Remaining": "0", "X-RateLimit-Reset": fmt.Sprint(now.Add(time.Hour).Unix())}, body: `{"message":"rate limit exceeded"}`, want: "github-primary-rate-limit"},
		{name: "secondary", headers: map[string]string{"Retry-After": "90", "X-GitHub-Request-Id": "request"}, body: `{"message":"secondary rate limit"}`, want: "github-secondary-rate-limit"},
		{name: "proxy", headers: map[string]string{}, body: `<html>access denied</html>`, want: "proxy-forbidden"},
		{name: "github forbidden", headers: map[string]string{"X-GitHub-Request-Id": "request"}, body: `{"message":"forbidden"}`, want: "github-forbidden"},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			failure := classifyCardUpdateHTTPFailure(response(test.headers, test.body), []byte(test.body), now)
			if failure.Code != test.want {
				t.Fatalf("code = %q, want %q", failure.Code, test.want)
			}
		})
	}
}

func TestCardUpdateCoordinatorCachesSuccessfulCheck(t *testing.T) {
	var calls atomic.Int32
	release := cardUpdateTestRelease("v4.0.5")
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		calls.Add(1)
		w.Header().Set("ETag", `"release-405"`)
		_ = json.NewEncoder(w).Encode(release)
	}))
	defer server.Close()

	cacheDir := t.TempDir()
	coordinator := newCardUpdateCoordinator()
	coordinator.apiURL = server.URL
	coordinator.pageURL = server.URL + "/latest"
	coordinator.path = func() string { return cacheDir + "/update-state.json" }
	first, failure := coordinator.result(false)
	if failure != nil || first.Cache.Release.TagName != "v4.0.5" {
		t.Fatalf("first result = %#v, failure=%v", first, failure)
	}
	second, failure := coordinator.result(false)
	if failure != nil || second.Source != "cache" {
		t.Fatalf("second result = %#v, failure=%v", second, failure)
	}
	restarted := newCardUpdateCoordinator()
	restarted.apiURL = server.URL
	restarted.pageURL = server.URL + "/latest"
	restarted.path = coordinator.path
	third, failure := restarted.result(false)
	if failure != nil || third.Source != "cache" {
		t.Fatalf("restart result = %#v, failure=%v", third, failure)
	}
	if got := calls.Load(); got != 1 {
		t.Fatalf("upstream calls = %d, want 1", got)
	}
}

func TestCardUpdateCoordinatorUsesStaleResultAndCooldown(t *testing.T) {
	var calls atomic.Int32
	now := time.Date(2026, 9, 4, 12, 0, 0, 0, time.UTC)
	release := cardUpdateTestRelease("v4.0.5")
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		if calls.Add(1) == 1 {
			_ = json.NewEncoder(w).Encode(release)
			return
		}
		w.Header().Set("X-RateLimit-Remaining", "0")
		w.Header().Set("X-RateLimit-Reset", fmt.Sprint(now.Add(time.Hour).Unix()))
		w.WriteHeader(http.StatusForbidden)
		_, _ = w.Write([]byte(`{"message":"rate limit exceeded"}`))
	}))
	defer server.Close()

	cacheDir := t.TempDir()
	coordinator := newCardUpdateCoordinator()
	coordinator.apiURL = server.URL + "/api"
	coordinator.pageURL = server.URL + "/latest"
	coordinator.path = func() string { return cacheDir + "/update-state.json" }
	coordinator.now = func() time.Time { return now }
	if _, failure := coordinator.result(false); failure != nil {
		t.Fatal(failure)
	}
	now = now.Add(7 * time.Hour)
	result, failure := coordinator.result(true)
	if failure != nil || result.Source != "stale-cache" || result.Warning == nil || result.Warning.Code != "github-primary-rate-limit" {
		t.Fatalf("stale result = %#v, failure=%v", result, failure)
	}
	callsAfterFailure := calls.Load()
	result, failure = coordinator.result(true)
	if failure != nil || result.Warning == nil || result.Warning.Code != "github-primary-rate-limit" {
		t.Fatalf("cooldown result = %#v, failure=%v", result, failure)
	}
	if got := calls.Load(); got != callsAfterFailure {
		t.Fatalf("cooldown made another upstream request: before=%d after=%d", callsAfterFailure, got)
	}
}

func TestCardUpdateCacheRejectsUntrustedAssetURL(t *testing.T) {
	release := cardUpdateTestRelease("v4.0.5")
	release.Assets[0].BrowserDownloadURL = "https://example.com/update.zip"
	if err := validateCardRelease(release); err == nil {
		t.Fatal("untrusted asset URL was accepted")
	}
}

func TestDiscoverCardReleaseViaOfficialPage(t *testing.T) {
	var calls atomic.Int32
	client := &http.Client{Transport: cardUpdateRoundTripFunc(func(request *http.Request) (*http.Response, error) {
		calls.Add(1)
		if request.Method == http.MethodGet {
			request.URL, _ = request.URL.Parse("https://github.com/Eric-Hou1997/Tech-Card-Manager/releases/tag/v4.0.5")
		}
		return &http.Response{StatusCode: http.StatusOK, Header: make(http.Header), Body: io.NopCloser(strings.NewReader("ok")), Request: request}, nil
	})}
	release, failure := discoverCardReleaseViaPage(client, cardReleasePage, time.Now())
	if failure != nil || release.TagName != "v4.0.5" {
		t.Fatalf("release=%#v failure=%v", release, failure)
	}
	if got := calls.Load(); got != 2 {
		t.Fatalf("requests=%d, want page plus asset probe", got)
	}
}
