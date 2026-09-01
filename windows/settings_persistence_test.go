//go:build windows

package main

import "testing"

func TestLanguageDefaultsAndValidation(t *testing.T) {
	if got := normalizedLanguage(""); got != "zh-CN" {
		t.Fatalf("empty language = %q, want zh-CN", got)
	}
	if got := normalizedLanguage("en-US"); got != "en-US" {
		t.Fatalf("en-US language = %q", got)
	}
	if got := normalizedLanguage("fr-FR"); got != "zh-CN" {
		t.Fatalf("unsupported language must fail closed to zh-CN, got %q", got)
	}
}

func TestConfiguredSettingsPreserveStartupPreferences(t *testing.T) {
	settings, err := validatedConfiguredSettings(Settings{
		IntervalSeconds: 60,
		RootsConfigured: true,
		LibraryRoots:    []LibraryRoot{{Path: `D:\Movies`, Kind: "movies", Source: "manual", Enabled: true}},
		AutoStart:       true,
		SilentStart:     true,
		Language:        "en-US",
	})
	if err != nil {
		t.Fatal(err)
	}
	if !settings.AutoStart || !settings.SilentStart || settings.Language != "en-US" {
		t.Fatalf("startup/language preferences were lost: %#v", settings)
	}
}

func TestCurrentLibraryRefreshNeverIncludesOtherOrMixedRoots(t *testing.T) {
	roots := []LibraryRoot{
		{Path: `D:\Movies`, Kind: "movies", Enabled: true},
		{Path: `D:\TV`, Kind: "tv", Enabled: true},
		{Path: `D:\Mixed`, Kind: "mixed", Enabled: true},
		{Path: `D:\DisabledMovies`, Kind: "movies", Enabled: false},
	}
	movies, mixed := enabledRootsForSpace(roots, "movies")
	if len(movies) != 1 || movies[0].Path != `D:\Movies` || mixed != 1 {
		t.Fatalf("movie refresh escaped its exact scope: roots=%#v mixed=%d", movies, mixed)
	}
	tv, mixed := enabledRootsForSpace(roots, "tv")
	if len(tv) != 1 || tv[0].Path != `D:\TV` || mixed != 1 {
		t.Fatalf("TV refresh escaped its exact scope: roots=%#v mixed=%d", tv, mixed)
	}
}
