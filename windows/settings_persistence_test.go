//go:build windows

package main

import (
	"bytes"
	"strings"
	"testing"
)

func TestLanguageDefaultsAndValidation(t *testing.T) {
	if got := normalizedLanguage(""); got != "zh-CN" {
		t.Fatalf("empty language = %q, want zh-CN", got)
	}
	if got := normalizedLanguage("en-US"); got != "en-US" {
		t.Fatalf("en-US language = %q", got)
	}
	if got := normalizedLanguage("zh-TW"); got != "zh-Hant" {
		t.Fatalf("Traditional Chinese alias was not normalized: %q", got)
	}
	if got := normalizedLanguage("fr-FR"); got != "zh-CN" {
		t.Fatalf("uninstalled language must fail closed to zh-CN, got %q", got)
	}
	if got := configuredLanguage("fr-FR"); got != "fr-FR" {
		t.Fatalf("registered language preference must survive an app upgrade, got %q", got)
	}
	if got := configuredLanguage("ko-KR"); got != "zh-CN" {
		t.Fatalf("unknown configured language must fail closed, got %q", got)
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

func TestLocalizedLineWriterFreezesLanguageAcrossSplitWrites(t *testing.T) {
	var output bytes.Buffer
	writer := newLocalizedLineWriter(&output, "en-US")
	if _, err := writer.Write([]byte("任务：运行")); err != nil {
		t.Fatal(err)
	}
	if _, err := writer.Write([]byte("诊断\n完成")); err != nil {
		t.Fatal(err)
	}
	if err := writer.Flush(); err != nil {
		t.Fatal(err)
	}
	if outputLanguageForWriter(writer) != "en-US" {
		t.Fatalf("writer language changed: %q", outputLanguageForWriter(writer))
	}
	if got := output.String(); strings.ContainsAny(got, "任务运行诊断完成") || !strings.Contains(got, "Task") || !strings.Contains(got, "Run diagnostics") || !strings.Contains(got, "Completed") {
		t.Fatalf("unexpected localized output: %q", got)
	}
}

func TestEngineOutputLanguageRemainsBilingualForPackLocales(t *testing.T) {
	if got := engineOutputLanguageForWriter(newLocalizedLineWriter(&bytes.Buffer{}, "zh-CN")); got != "zh-CN" {
		t.Fatalf("Simplified Chinese engine language = %q", got)
	}
	for _, language := range []string{"zh-Hant", "en-US", "fr-FR", "ru-RU", "ja-JP", "es-ES", "th-TH"} {
		writer := &localizedLineWriter{dst: &bytes.Buffer{}, language: language}
		if got := engineOutputLanguageForWriter(writer); got != "en-US" {
			t.Fatalf("%s engine language = %q, want en-US", language, got)
		}
	}
}

func TestBackendLocalizationPreservesChineseWindowsPaths(t *testing.T) {
	path := `D:\电影\完成\任务.nfo`
	got := localizeBackendText("en-US", "NFO XML 解析失败，已跳过："+path)
	if !strings.Contains(got, path) {
		t.Fatalf("localized task output changed the user path: %q", got)
	}
	if !strings.HasPrefix(got, "NFO XML parsing failed and was skipped") {
		t.Fatalf("product-owned message was not localized: %q", got)
	}
}

func TestStatusLocalizationPreservesPathsProtocolAndHistoricalJob(t *testing.T) {
	status := Status{
		Language: "en-US",
		Service:  ServiceSnapshot{Message: "服务运行中", Legacy: LegacyReport{Items: []string{"旧版后台 Agent 正在运行"}}},
		Job:      JobState{Language: "zh-CN", Message: "完成", Log: "任务：运行诊断\n完成\n"},
		Libraries: []LibraryInfo{{
			Path: `D:\电影\完成`, Evidence: "用户选择",
		}},
		Extra: map[string]interface{}{"index_summary_error": "Manager 索引摘要尚未生成", "protocol": "Camera"},
	}
	localizeStatusForPresentation(&status)
	if status.Service.Message != "Service running" || strings.Contains(status.Service.Legacy.Items[0], "旧版") {
		t.Fatalf("service presentation was not localized: %#v", status.Service)
	}
	if status.Libraries[0].Path != `D:\电影\完成` || status.Extra["protocol"] != "Camera" {
		t.Fatalf("user path or protocol changed: %#v %#v", status.Libraries, status.Extra)
	}
	if status.Job.Language != "zh-CN" || status.Job.Message != "完成" || !strings.Contains(status.Job.Log, "任务：运行诊断") {
		t.Fatalf("historical job was rewritten: %#v", status.Job)
	}
}
