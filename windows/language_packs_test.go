//go:build windows

package main

import (
	"archive/zip"
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

type languageRoundTripper func(*http.Request) (*http.Response, error)

func (fn languageRoundTripper) RoundTrip(request *http.Request) (*http.Response, error) {
	return fn(request)
}

func testLanguageArchive(t *testing.T, product, locale string, descriptor LanguagePackDescriptor) []byte {
	t.Helper()
	files := map[string][]byte{"web.json": []byte(`{"legacy.test":"Web"}`), "core.json": []byte(`{"legacy.test":"Core"}`), "engine.json": []byte(`{}`), "native.json": []byte(`{}`), "web-card.json": []byte(`{}`)}
	fileHashes := make(map[string]string, len(files))
	for name, payload := range files {
		sum := sha256.Sum256(payload)
		fileHashes[name] = hex.EncodeToString(sum[:])
	}
	manifest, _ := json.Marshal(installedLanguageManifest{Schema: 1, Product: product, Locale: locale, Revision: descriptor.Revision, ReleasedWith: descriptor.ReleasedWith, CatalogSchema: descriptor.CatalogSchema, MessageSetHash: descriptor.MessageSetHash, Files: fileHashes})
	files["manifest.json"] = manifest
	var buffer bytes.Buffer
	writer := zip.NewWriter(&buffer)
	for _, name := range []string{"manifest.json", "web.json", "core.json", "engine.json", "native.json", "web-card.json"} {
		entry, err := writer.Create(name)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := entry.Write(files[name]); err != nil {
			t.Fatal(err)
		}
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	return buffer.Bytes()
}

func TestLanguagePackDownloadInstallAndExactCatalogBinding(t *testing.T) {
	languagePackRootOverride = t.TempDir()
	originalCatalog := languageCatalog
	originalClient := languagePackHTTPClient
	originalPublisher := publishWebCardLanguages
	t.Cleanup(func() {
		languagePackRootOverride = ""
		languageCatalog = originalCatalog
		languagePackHTTPClient = originalClient
		publishWebCardLanguages = originalPublisher
		languagePackCache = map[string]map[string]map[string]string{}
		languagePackErrors = map[string]string{}
	})
	publishWebCardLanguages = func() error { return nil }
	languageCatalog.Languages = cloneLanguageDescriptors(originalCatalog.Languages)
	languageCatalog.AppVersion = "v" + appVersion
	descriptor := LanguagePackDescriptor{Revision: 7, ReleasedWith: "v4.1.0", Asset: "TCM-Language-fr-FR-r7.zip", CatalogSchema: 1, MessageSetHash: strings.Repeat("1", 64)}
	payload := testLanguageArchive(t, languagePackProduct, "fr-FR", descriptor)
	digest := sha256.Sum256(payload)
	descriptor.SHA256 = hex.EncodeToString(digest[:])
	languageCatalog.Languages["fr-FR"] = descriptor
	languagePackHTTPClient = &http.Client{Transport: languageRoundTripper(func(request *http.Request) (*http.Response, error) {
		if !strings.Contains(request.URL.Path, "/releases/download/v4.1.0/TCM-Language-fr-FR-r7.zip") {
			t.Fatalf("unexpected URL %s", request.URL)
		}
		return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(bytes.NewReader(payload)), Header: make(http.Header)}, nil
	}), Timeout: time.Second}
	if err := installLanguagePack("fr-FR"); err != nil {
		t.Fatal(err)
	}
	if normalizedLanguage("fr-FR") != "fr-FR" {
		t.Fatal("installed language was not selectable")
	}
	if value, ok := languagePackMessage("fr-FR", "core", "legacy.test"); !ok || value != "Core" {
		t.Fatalf("installed catalog not loaded: %q %v", value, ok)
	}
	if err := os.WriteFile(filepath.Join(languagePackPath("fr-FR", descriptor), "manifest.json"), []byte(`{"broken":true}`), 0644); err != nil {
		t.Fatal(err)
	}
	if err := installLanguagePack("fr-FR"); err != nil || !languagePackInstalled("fr-FR") {
		t.Fatalf("a corrupt exact-revision directory was not atomically replaced: %v", err)
	}
	if err := os.WriteFile(filepath.Join(languagePackPath("fr-FR", descriptor), "web.json"), []byte(`{"legacy.test":"Tampered"}`), 0644); err != nil {
		t.Fatal(err)
	}
	if languagePackInstalled("fr-FR") {
		t.Fatal("a modified extracted translation file passed its manifest hash")
	}
	if err := installLanguagePack("fr-FR"); err != nil || !languagePackInstalled("fr-FR") {
		t.Fatalf("a modified exact-revision translation was not atomically restored: %v", err)
	}
	changed := descriptor
	changed.Revision = 8
	languageCatalog.Languages["fr-FR"] = changed
	if normalizedLanguage("fr-FR") != defaultLanguage {
		t.Fatal("a pack from another app catalog revision was reused")
	}
}

func TestLanguagePackRejectsWrongProduct(t *testing.T) {
	languagePackRootOverride = t.TempDir()
	t.Cleanup(func() { languagePackRootOverride = "" })
	descriptor := LanguagePackDescriptor{Revision: 1, ReleasedWith: "v4.1.0", Asset: "TCM-Language-ja-JP-r1.zip", CatalogSchema: 1, MessageSetHash: strings.Repeat("1", 64)}
	payload := testLanguageArchive(t, "wrong-product", "ja-JP", descriptor)
	if err := extractLanguagePack(payload, "ja-JP", descriptor); err == nil {
		t.Fatal("wrong-product pack was accepted")
	}
}

func TestLanguagePackCatalogIsInactiveForAnotherAppVersion(t *testing.T) {
	originalCatalog := languageCatalog
	languageCatalog.AppVersion = "v99.0.0"
	t.Cleanup(func() { languageCatalog = originalCatalog })
	for _, option := range supportedLanguages() {
		if !option.BuiltIn && option.Downloadable {
			t.Fatalf("future catalog exposed %s as downloadable", option.Code)
		}
	}
	if err := installLanguagePack("fr-FR"); err == nil || !strings.Contains(err.Error(), "不属于当前应用版本") {
		t.Fatalf("future catalog installation was not rejected: %v", err)
	}
}

func TestLanguagePackRedirectsStayOnOfficialHosts(t *testing.T) {
	client := newLanguagePackHTTPClient()
	official, _ := http.NewRequest(http.MethodGet, "https://release-assets.githubusercontent.com/example", nil)
	if err := client.CheckRedirect(official, []*http.Request{{}}); err != nil {
		t.Fatalf("official GitHub asset redirect was rejected: %v", err)
	}
	unofficial, _ := http.NewRequest(http.MethodGet, "https://example.invalid/language.zip", nil)
	if err := client.CheckRedirect(unofficial, []*http.Request{{}}); err == nil {
		t.Fatal("unofficial language-pack redirect was accepted")
	}
}

func TestLanguagePackHistoryDetectionDrivesUpgradeRestoration(t *testing.T) {
	languagePackRootOverride = t.TempDir()
	t.Cleanup(func() { languagePackRootOverride = "" })
	if languagePackHistoryPresent("ja-JP") {
		t.Fatal("a never-installed language was treated as upgrade state")
	}
	if err := os.MkdirAll(filepath.Join(languagePackRoot(), "ja-JP", "r1"), 0755); err != nil {
		t.Fatal(err)
	}
	if !languagePackHistoryPresent("ja-JP") {
		t.Fatal("a prior language revision was not detected for automatic restoration")
	}
}

func TestLanguagePackRestorePlanIncludesCurrentAndPreviouslyInstalledOnly(t *testing.T) {
	languagePackRootOverride = t.TempDir()
	originalCatalog := languageCatalog
	languageCatalog.Languages = cloneLanguageDescriptors(originalCatalog.Languages)
	languageCatalog.AppVersion = "v" + appVersion
	t.Cleanup(func() {
		languagePackRootOverride = ""
		languageCatalog = originalCatalog
	})
	if err := os.MkdirAll(filepath.Join(languagePackRoot(), "ja-JP", "r1"), 0755); err != nil {
		t.Fatal(err)
	}
	got := strings.Join(languagePacksNeedingRestore("fr-FR"), ",")
	if got != "fr-FR,ja-JP" {
		t.Fatalf("restore plan = %q, want configured fr-FR and prior ja-JP only", got)
	}
	languageCatalog.AppVersion = "v99.0.0"
	if got := languagePacksNeedingRestore("fr-FR"); len(got) != 0 {
		t.Fatalf("inactive catalog produced a restore plan: %#v", got)
	}
}

func cloneLanguageDescriptors(source map[string]LanguagePackDescriptor) map[string]LanguagePackDescriptor {
	result := make(map[string]LanguagePackDescriptor, len(source))
	for key, value := range source {
		result[key] = value
	}
	return result
}
