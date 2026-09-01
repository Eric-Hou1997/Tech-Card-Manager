//go:build windows

package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestWithUTF8BOM(t *testing.T) {
	bom := []byte{0xEF, 0xBB, 0xBF}
	plain := []byte("param()")
	got := withUTF8BOM(plain)
	if !bytes.HasPrefix(got, bom) {
		t.Fatal("PowerShell asset must be written with a UTF-8 BOM")
	}
	if twice := withUTF8BOM(got); !bytes.Equal(twice, got) {
		t.Fatal("BOM enforcement must be idempotent")
	}
}

func TestSanitizeLibraryRoots(t *testing.T) {
	roots, err := sanitizeLibraryRoots([]LibraryRoot{
		{Path: `D:\Movies`, Name: "电影", Kind: "movies", Source: "auto", Enabled: true},
		{Path: `\\NAS\TV`, Name: "电视剧", Kind: "tv", Source: "manual", Enabled: true},
	})
	if err != nil {
		t.Fatal(err)
	}
	if len(roots) != 2 || roots[0].Kind != "movies" || roots[1].Source != "manual" {
		t.Fatalf("unexpected roots: %#v", roots)
	}
}

func TestSanitizeLibraryRootsRejectsDriveRootAndDuplicates(t *testing.T) {
	if _, err := sanitizeLibraryRoots([]LibraryRoot{{Path: `D:\`, Enabled: true}}); err == nil {
		t.Fatal("whole drive roots must be rejected")
	}
	if _, err := sanitizeLibraryRoots([]LibraryRoot{
		{Path: `D:\Movies`, Enabled: true},
		{Path: `d:\movies\`, Enabled: true},
	}); err == nil {
		t.Fatal("case-insensitive duplicate roots must be rejected")
	}
	if _, err := sanitizeLibraryRoots([]LibraryRoot{
		{Path: `D:\Media`, Enabled: true},
		{Path: `D:\Media\Movies`, Enabled: true},
	}); err == nil {
		t.Fatal("nested roots must be rejected to prevent duplicate scans")
	}
}

func TestValidatedConfiguredSettingsAndRecoveredKinds(t *testing.T) {
	settings, err := validatedConfiguredSettings(Settings{
		IntervalSeconds: 10,
		RootsConfigured: true,
		LibraryRoots:    []LibraryRoot{{Path: `D:\Movies`, Kind: "movies", Source: "auto", Enabled: true}},
	})
	if err != nil {
		t.Fatal(err)
	}
	if settings.IntervalSeconds != 60 || settings.AutoStart || len(settings.LibraryRoots) != 1 {
		t.Fatalf("unexpected recovered settings: %#v", settings)
	}
	if recoveredKind("Mixed Movie/TV") != "mixed" || recoveredKind("Series") != "tv" || recoveredKind("Movies") != "movies" {
		t.Fatal("legacy index kinds must normalize to current settings values")
	}
}

func TestDerivedIndexesMustMatchSchemaGenerationAndRoots(t *testing.T) {
	appData := t.TempDir()
	t.Setenv("APPDATA", appData)
	dashboard := filepath.Join(appData, "Emby-Server", "system", "dashboard-ui")
	custom := filepath.Join(appData, "Emby-Server", "programdata", "custom-tech-specs")
	if err := os.MkdirAll(dashboard, 0755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(custom, 0755); err != nil {
		t.Fatal(err)
	}
	writeJSON := func(path string, value interface{}) {
		b, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, b, 0644); err != nil {
			t.Fatal(err)
		}
	}
	writeJSON(techDataFile(), map[string]interface{}{
		"version": 7, "generatedAt": "2026-08-24T00:00:00Z",
		"items": map[string]interface{}{}, "itemTypes": map[string]string{},
	})
	writeJSON(managerCatalogFile(), map[string]interface{}{"items": []interface{}{}})
	writeJSON(indexSummaryFile(), map[string]interface{}{
		"generatedAt": "2026-08-24T00:00:00Z", "libraryRoots": []string{`D:\Movies`},
		"scanStats": map[string]int{}, "items": map[string]interface{}{},
	})
	settings := Settings{RootsConfigured: true, LibraryRoots: []LibraryRoot{{Path: `D:\Movies`, Enabled: true}}}
	if !derivedIndexesValidForSettings(settings) {
		t.Fatal("matching v7 index set should be ready")
	}
	settings.LibraryRoots[0].Path = `D:\TV`
	if derivedIndexesValidForSettings(settings) {
		t.Fatal("index from a different configured root must not receive a live card lease")
	}
}

func TestElevatedRequestIDValidation(t *testing.T) {
	valid := "0123456789abcdef0123456789abcdef"
	if !validElevatedRequestID(valid) {
		t.Fatal("32-character lowercase hex request id must be accepted")
	}
	for _, invalid := range []string{
		"",
		"0123456789abcdef",
		"0123456789abcdef0123456789abcdeg",
		"0123456789ABCDEF0123456789ABCDEF",
		"../../elevated-result.json",
	} {
		if validElevatedRequestID(invalid) {
			t.Fatalf("unsafe request id accepted: %q", invalid)
		}
	}
}
