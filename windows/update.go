package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"time"
)

const (
	cardReleaseAPI       = "https://api.github.com/repos/Eric-Hou1997/Tech-Card-Manager/releases/latest"
	cardReleasePage      = "https://github.com/Eric-Hou1997/Tech-Card-Manager/releases/latest"
	cardUpdateCacheTTL   = 6 * time.Hour
	cardUpdateStaleLimit = 30 * 24 * time.Hour
)

var cardVersionPattern = regexp.MustCompile(`^v?(\d+)\.(\d+)\.(\d+)$`)

type cardGitHubRelease struct {
	TagName     string                   `json:"tag_name"`
	HTMLURL     string                   `json:"html_url"`
	Draft       bool                     `json:"draft"`
	Prerelease  bool                     `json:"prerelease"`
	PublishedAt string                   `json:"published_at"`
	Assets      []cardGitHubReleaseAsset `json:"assets"`
}

type cardGitHubReleaseAsset struct {
	Name               string `json:"name"`
	BrowserDownloadURL string `json:"browser_download_url"`
}

type cardUpdateCache struct {
	SchemaVersion int               `json:"schema_version"`
	ETag          string            `json:"etag,omitempty"`
	CheckedAt     time.Time         `json:"checked_at"`
	Release       cardGitHubRelease `json:"release"`
}

type cardUpdateFailure struct {
	Code      string
	Status    int
	RetryAt   time.Time
	RequestID string
	Chinese   string
	English   string
}

func (failure *cardUpdateFailure) Error() string {
	if failure == nil {
		return ""
	}
	return currentLocalized(failure.Chinese, failure.English)
}

type cardUpdateResult struct {
	Cache   cardUpdateCache
	Source  string
	Warning *cardUpdateFailure
}

type cardUpdateCoordinator struct {
	mu       sync.Mutex
	loaded   bool
	cache    *cardUpdateCache
	inFlight chan struct{}
	retryAt  time.Time
	retryErr *cardUpdateFailure
	now      func() time.Time
	path     func() string
	apiURL   string
	pageURL  string
}

var cardUpdates = newCardUpdateCoordinator()

func newCardUpdateCoordinator() *cardUpdateCoordinator {
	return &cardUpdateCoordinator{
		now:     time.Now,
		path:    func() string { return filepath.Join(platformDataPath(), "update-state.json") },
		apiURL:  cardReleaseAPI,
		pageURL: cardReleasePage,
	}
}

func cardGitHubClient(timeout time.Duration) *http.Client {
	return &http.Client{
		Timeout: timeout,
		CheckRedirect: func(req *http.Request, via []*http.Request) error {
			if len(via) >= 10 {
				return errors.New("GitHub 更新重定向次数过多")
			}
			host := strings.ToLower(req.URL.Hostname())
			if host != "github.com" && host != "api.github.com" && !strings.HasSuffix(host, ".githubusercontent.com") {
				return fmt.Errorf("GitHub 更新重定向到非官方主机 %s，已拒绝", host)
			}
			return nil
		},
	}
}

func cardUpdateRequest(method, rawURL string) (*http.Request, error) {
	req, err := http.NewRequest(method, rawURL, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Accept", "application/vnd.github+json")
	req.Header.Set("X-GitHub-Api-Version", "2022-11-28")
	req.Header.Set("User-Agent", "Tech-Card-Manager/"+appVersion)
	return req, nil
}

func cardUpdateRetryAt(resp *http.Response, now time.Time) time.Time {
	if reset, err := strconv.ParseInt(strings.TrimSpace(resp.Header.Get("X-RateLimit-Reset")), 10, 64); err == nil && reset > 0 {
		return time.Unix(reset, 0)
	}
	if value := strings.TrimSpace(resp.Header.Get("Retry-After")); value != "" {
		if seconds, err := strconv.Atoi(value); err == nil && seconds >= 0 {
			return now.Add(time.Duration(seconds) * time.Second)
		}
		if parsed, err := http.ParseTime(value); err == nil {
			return parsed
		}
	}
	return time.Time{}
}

func classifyCardUpdateHTTPFailure(resp *http.Response, body []byte, now time.Time) *cardUpdateFailure {
	status := resp.StatusCode
	requestID := strings.TrimSpace(resp.Header.Get("X-GitHub-Request-Id"))
	retryAt := cardUpdateRetryAt(resp, now)
	lower := strings.ToLower(string(body))
	remaining := strings.TrimSpace(resp.Header.Get("X-RateLimit-Remaining"))
	if (status == http.StatusForbidden || status == http.StatusTooManyRequests) && remaining == "0" {
		if retryAt.IsZero() {
			retryAt = now.Add(time.Minute)
		}
		return &cardUpdateFailure{Code: "github-primary-rate-limit", Status: status, RetryAt: retryAt, RequestID: requestID,
			Chinese: "GitHub 匿名 API 的出口 IP 额度已用完", English: "The GitHub anonymous API quota for this public IP has been exhausted"}
	}
	if (status == http.StatusForbidden || status == http.StatusTooManyRequests) &&
		(resp.Header.Get("Retry-After") != "" || strings.Contains(lower, "secondary rate limit") || strings.Contains(lower, "abuse detection")) {
		if retryAt.IsZero() {
			retryAt = now.Add(time.Minute)
		}
		return &cardUpdateFailure{Code: "github-secondary-rate-limit", Status: status, RetryAt: retryAt, RequestID: requestID,
			Chinese: "GitHub 触发了次级限流，请按提示时间后再检查", English: "GitHub applied a secondary rate limit; check again after the indicated time"}
	}
	if status == http.StatusForbidden || status == http.StatusTooManyRequests {
		if requestID == "" && !strings.Contains(lower, "api.github.com") && !strings.Contains(lower, "github") {
			return &cardUpdateFailure{Code: "proxy-forbidden", Status: status,
				Chinese: "代理或中间网络拒绝了更新请求", English: "A proxy or intermediary network rejected the update request"}
		}
		return &cardUpdateFailure{Code: "github-forbidden", Status: status, RequestID: requestID,
			Chinese: "GitHub 拒绝了更新请求，但未标明为额度耗尽", English: "GitHub rejected the update request without identifying an exhausted quota"}
	}
	if status >= 500 {
		return &cardUpdateFailure{Code: "github-unavailable", Status: status, RequestID: requestID,
			Chinese: "GitHub 更新服务暂时不可用", English: "The GitHub update service is temporarily unavailable"}
	}
	return &cardUpdateFailure{Code: "github-http-error", Status: status, RequestID: requestID,
		Chinese: fmt.Sprintf("GitHub 更新请求失败（HTTP %d）", status), English: fmt.Sprintf("The GitHub update request failed (HTTP %d)", status)}
}

func classifyCardUpdateNetworkFailure(err error) *cardUpdateFailure {
	lower := strings.ToLower(err.Error())
	if strings.Contains(lower, "proxy") {
		return &cardUpdateFailure{Code: "proxy-connection-failed", Chinese: "无法连接代理服务器：" + err.Error(), English: "Could not connect to the proxy server: " + err.Error()}
	}
	return &cardUpdateFailure{Code: "network-unavailable", Chinese: "无法连接 GitHub：" + err.Error(), English: "Could not connect to GitHub: " + err.Error()}
}

func readCardUpdateErrorBody(resp *http.Response) []byte {
	body, _ := io.ReadAll(io.LimitReader(resp.Body, 64<<10))
	return body
}

func validateCardRelease(release cardGitHubRelease) error {
	if release.Draft || release.Prerelease || cardVersionPattern.FindStringSubmatch(release.TagName) == nil {
		return errors.New("GitHub 最新发布不是可用的正式 vX.Y.Z 版本")
	}
	archive, err := selectCardUpdateAsset(release)
	if err != nil {
		return err
	}
	if !officialCardAssetURL(archive.BrowserDownloadURL, release.TagName, archive.Name) {
		return errors.New("正式发布返回了非官方或不匹配的更新地址")
	}
	return nil
}

func officialCardAssetURL(rawURL, tag, name string) bool {
	parsed, err := url.Parse(rawURL)
	if err != nil || parsed.Scheme != "https" || !strings.EqualFold(parsed.Hostname(), "github.com") || parsed.RawQuery != "" || parsed.Fragment != "" {
		return false
	}
	want := "/Eric-Hou1997/Tech-Card-Manager/releases/download/" + url.PathEscape(tag) + "/" + url.PathEscape(name)
	return parsed.EscapedPath() == want
}

func (coordinator *cardUpdateCoordinator) loadCacheLocked() {
	if coordinator.loaded {
		return
	}
	coordinator.loaded = true
	body, err := os.ReadFile(coordinator.path())
	if err != nil {
		return
	}
	var cached cardUpdateCache
	if json.Unmarshal(body, &cached) != nil || cached.SchemaVersion != 1 || validateCardRelease(cached.Release) != nil {
		return
	}
	now := coordinator.now()
	if cached.CheckedAt.IsZero() || cached.CheckedAt.After(now.Add(5*time.Minute)) {
		return
	}
	coordinator.cache = &cached
}

func (coordinator *cardUpdateCoordinator) saveCache(cache cardUpdateCache) error {
	body, err := json.MarshalIndent(cache, "", "  ")
	if err != nil {
		return err
	}
	return atomicWrite(coordinator.path(), body, 0600)
}

func fetchCardReleaseAPI(client *http.Client, rawURL, etag string, now time.Time) (cardGitHubRelease, string, bool, *cardUpdateFailure) {
	req, err := cardUpdateRequest(http.MethodGet, rawURL)
	if err != nil {
		return cardGitHubRelease{}, "", false, classifyCardUpdateNetworkFailure(err)
	}
	if etag != "" {
		req.Header.Set("If-None-Match", etag)
	}
	resp, err := client.Do(req)
	if err != nil {
		return cardGitHubRelease{}, "", false, classifyCardUpdateNetworkFailure(err)
	}
	defer resp.Body.Close()
	if resp.StatusCode == http.StatusNotModified {
		return cardGitHubRelease{}, etag, true, nil
	}
	if resp.StatusCode != http.StatusOK {
		return cardGitHubRelease{}, "", false, classifyCardUpdateHTTPFailure(resp, readCardUpdateErrorBody(resp), now)
	}
	var release cardGitHubRelease
	if err := json.NewDecoder(io.LimitReader(resp.Body, 2<<20)).Decode(&release); err != nil {
		return cardGitHubRelease{}, "", false, &cardUpdateFailure{Code: "invalid-release-response", Chinese: "GitHub 更新信息无效：" + err.Error(), English: "The GitHub update response is invalid: " + err.Error()}
	}
	if err := validateCardRelease(release); err != nil {
		return cardGitHubRelease{}, "", false, &cardUpdateFailure{Code: "invalid-release", Chinese: err.Error(), English: localizeBackendText("en-US", err.Error())}
	}
	return release, strings.TrimSpace(resp.Header.Get("ETag")), false, nil
}

func cardReleaseTagFromURL(rawURL string) (string, bool) {
	parsed, err := url.Parse(rawURL)
	if err != nil || !strings.EqualFold(parsed.Hostname(), "github.com") {
		return "", false
	}
	prefix := "/Eric-Hou1997/Tech-Card-Manager/releases/tag/"
	if !strings.HasPrefix(parsed.EscapedPath(), prefix) {
		return "", false
	}
	tag, err := url.PathUnescape(strings.TrimPrefix(parsed.EscapedPath(), prefix))
	return tag, err == nil && cardVersionPattern.FindStringSubmatch(tag) != nil
}

func probeCardUpdateAsset(client *http.Client, rawURL string, now time.Time) *cardUpdateFailure {
	req, err := cardUpdateRequest(http.MethodHead, rawURL)
	if err != nil {
		return classifyCardUpdateNetworkFailure(err)
	}
	resp, err := client.Do(req)
	if err != nil {
		return classifyCardUpdateNetworkFailure(err)
	}
	defer resp.Body.Close()
	if resp.StatusCode == http.StatusOK || resp.StatusCode == http.StatusPartialContent {
		return nil
	}
	return classifyCardUpdateHTTPFailure(resp, readCardUpdateErrorBody(resp), now)
}

func discoverCardReleaseViaPage(client *http.Client, pageURL string, now time.Time) (cardGitHubRelease, *cardUpdateFailure) {
	req, err := cardUpdateRequest(http.MethodGet, pageURL)
	if err != nil {
		return cardGitHubRelease{}, classifyCardUpdateNetworkFailure(err)
	}
	req.Header.Set("Accept", "text/html")
	resp, err := client.Do(req)
	if err != nil {
		return cardGitHubRelease{}, classifyCardUpdateNetworkFailure(err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return cardGitHubRelease{}, classifyCardUpdateHTTPFailure(resp, readCardUpdateErrorBody(resp), now)
	}
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 2<<20))
	tag, ok := cardReleaseTagFromURL(resp.Request.URL.String())
	if !ok {
		return cardGitHubRelease{}, &cardUpdateFailure{Code: "invalid-release-redirect", Chinese: "GitHub 最新发布页面没有返回有效的正式版本", English: "The GitHub latest-release page did not return a valid stable version"}
	}
	archiveName, _ := cardUpdateArchiveName(tag)
	packageURL := "https://github.com/Eric-Hou1997/Tech-Card-Manager/releases/download/" + url.PathEscape(tag) + "/" + url.PathEscape(archiveName)
	if failure := probeCardUpdateAsset(client, packageURL, now); failure != nil {
		return cardGitHubRelease{}, failure
	}
	return cardGitHubRelease{TagName: tag, HTMLURL: resp.Request.URL.String(), Assets: []cardGitHubReleaseAsset{{Name: archiveName, BrowserDownloadURL: packageURL}}}, nil
}

func (coordinator *cardUpdateCoordinator) result(force bool) (cardUpdateResult, *cardUpdateFailure) {
	for {
		coordinator.mu.Lock()
		coordinator.loadCacheLocked()
		now := coordinator.now()
		if !force && coordinator.cache != nil && now.Sub(coordinator.cache.CheckedAt) < cardUpdateCacheTTL {
			result := cardUpdateResult{Cache: *coordinator.cache, Source: "cache"}
			coordinator.mu.Unlock()
			return result, nil
		}
		if coordinator.retryErr != nil && now.Before(coordinator.retryAt) {
			failure := *coordinator.retryErr
			if coordinator.cache != nil && now.Sub(coordinator.cache.CheckedAt) <= cardUpdateStaleLimit {
				result := cardUpdateResult{Cache: *coordinator.cache, Source: "stale-cache", Warning: &failure}
				coordinator.mu.Unlock()
				return result, nil
			}
			coordinator.mu.Unlock()
			return cardUpdateResult{}, &failure
		}
		if coordinator.inFlight != nil {
			wait := coordinator.inFlight
			coordinator.mu.Unlock()
			<-wait
			force = false
			continue
		}
		coordinator.inFlight = make(chan struct{})
		wait := coordinator.inFlight
		var stale *cardUpdateCache
		etag := ""
		if coordinator.cache != nil {
			copyCache := *coordinator.cache
			stale = &copyCache
			etag = copyCache.ETag
		}
		coordinator.mu.Unlock()

		client := cardGitHubClient(20 * time.Second)
		release, nextETag, notModified, failure := fetchCardReleaseAPI(client, coordinator.apiURL, etag, now)
		source := "github-api"
		if failure != nil {
			if fallback, fallbackFailure := discoverCardReleaseViaPage(client, coordinator.pageURL, now); fallbackFailure == nil {
				release, nextETag, notModified, source = fallback, "", false, "github-release-page"
				failure = nil
			} else if failure.Code == "network-unavailable" || failure.Code == "github-unavailable" || failure.Code == "github-http-error" {
				failure = fallbackFailure
			}
		}
		var result cardUpdateResult
		if failure == nil {
			if notModified {
				if stale == nil {
					failure = &cardUpdateFailure{Code: "invalid-not-modified", Chinese: "GitHub 返回了无法使用的未修改状态", English: "GitHub returned an unusable not-modified response"}
				} else {
					release = stale.Release
					nextETag = stale.ETag
				}
			}
		}
		if failure == nil {
			cache := cardUpdateCache{SchemaVersion: 1, ETag: nextETag, CheckedAt: now, Release: release}
			coordinator.mu.Lock()
			coordinator.cache = &cache
			coordinator.retryAt = time.Time{}
			coordinator.retryErr = nil
			coordinator.mu.Unlock()
			result = cardUpdateResult{Cache: cache, Source: source}
			if err := coordinator.saveCache(cache); err != nil {
				result.Warning = &cardUpdateFailure{Code: "update-cache-write-failed", Chinese: "已检查到更新，但无法保存更新状态：" + err.Error(), English: "The update was checked, but its state could not be saved: " + err.Error()}
			}
		}
		if failure != nil && stale != nil && now.Sub(stale.CheckedAt) <= cardUpdateStaleLimit {
			result = cardUpdateResult{Cache: *stale, Source: "stale-cache", Warning: failure}
			failure = nil
		}

		coordinator.mu.Lock()
		cooldownFailure := failure
		if cooldownFailure == nil {
			cooldownFailure = result.Warning
		}
		if cooldownFailure != nil && !cooldownFailure.RetryAt.IsZero() {
			copyFailure := *cooldownFailure
			coordinator.retryAt = cooldownFailure.RetryAt
			coordinator.retryErr = &copyFailure
		}
		if coordinator.inFlight == wait {
			coordinator.inFlight = nil
			close(wait)
		}
		coordinator.mu.Unlock()
		return result, failure
	}
}

func compareCardVersion(left, right string) (int, error) {
	lm, rm := cardVersionPattern.FindStringSubmatch(strings.TrimSpace(left)), cardVersionPattern.FindStringSubmatch(strings.TrimSpace(right))
	if lm == nil || rm == nil {
		return 0, errors.New("版本号必须为 vX.Y.Z")
	}
	for i := 1; i <= 3; i++ {
		if lm[i] == rm[i] {
			continue
		}
		var l, r int
		_, _ = fmt.Sscanf(lm[i], "%d", &l)
		_, _ = fmt.Sscanf(rm[i], "%d", &r)
		if l < r {
			return -1, nil
		}
		return 1, nil
	}
	return 0, nil
}

func cardUpdateArchiveName(tag string) (string, error) {
	match := cardVersionPattern.FindStringSubmatch(strings.TrimSpace(tag))
	if match == nil {
		return "", errors.New("版本号必须为 vX.Y.Z")
	}
	return fmt.Sprintf("TCM-v%s.%s.%s-Windows-x64-EXE.zip", match[1], match[2], match[3]), nil
}

func selectCardUpdateAsset(release cardGitHubRelease) (cardGitHubReleaseAsset, error) {
	expectedArchive, err := cardUpdateArchiveName(release.TagName)
	if err != nil {
		return cardGitHubReleaseAsset{}, err
	}
	for _, asset := range release.Assets {
		if asset.Name == expectedArchive {
			return asset, nil
		}
	}
	return cardGitHubReleaseAsset{}, fmt.Errorf("该正式发布缺少指定更新包 %s", expectedArchive)
}

func handleCardUpdate(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		writeJSONStatus(w, http.StatusMethodNotAllowed, map[string]string{"error": "仅支持 GET"})
		return
	}
	force := r.URL.Query().Get("force") == "1"
	result, failure := cardUpdates.result(force)
	if failure != nil {
		writeCardUpdateFailure(w, failure)
		return
	}
	release := result.Cache.Release
	archive, err := selectCardUpdateAsset(release)
	if err != nil {
		writeJSONStatus(w, http.StatusBadGateway, map[string]string{"error": err.Error()})
		return
	}
	cmp, err := compareCardVersion(appVersion, release.TagName)
	if err != nil {
		writeJSONStatus(w, http.StatusBadGateway, map[string]string{"error": err.Error()})
		return
	}
	releaseURL := strings.TrimSpace(release.HTMLURL)
	if releaseURL == "" {
		releaseURL = cardReleasePage
	}
	payload := map[string]interface{}{
		"current_version": "v" + appVersion, "latest_version": release.TagName, "available": cmp < 0,
		"release_url": releaseURL, "published_at": release.PublishedAt,
		"package_name": archive.Name, "package_url": archive.BrowserDownloadURL,
		"portable_directory": portableRootDir(),
		"source":             result.Source, "checked_at": result.Cache.CheckedAt.Format(time.RFC3339),
	}
	if result.Warning != nil {
		payload["warning_code"] = result.Warning.Code
		payload["warning"] = result.Warning.Error()
		if !result.Warning.RetryAt.IsZero() {
			payload["retry_at"] = result.Warning.RetryAt.Format(time.RFC3339)
		}
	}
	writeJSON(w, payload)
}

func writeCardUpdateFailure(w http.ResponseWriter, failure *cardUpdateFailure) {
	status := http.StatusBadGateway
	if failure.Code == "github-primary-rate-limit" || failure.Code == "github-secondary-rate-limit" {
		status = http.StatusTooManyRequests
	}
	payload := map[string]interface{}{"error": failure.Error(), "error_code": failure.Code}
	if failure.Status != 0 {
		payload["upstream_status"] = failure.Status
	}
	if !failure.RetryAt.IsZero() {
		payload["retry_at"] = failure.RetryAt.Format(time.RFC3339)
	}
	if failure.RequestID != "" {
		payload["github_request_id"] = failure.RequestID
	}
	writeJSONStatus(w, status, payload)
}
