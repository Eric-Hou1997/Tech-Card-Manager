package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"regexp"
	"strings"
	"time"
)

const (
	cardReleaseAPI  = "https://api.github.com/repos/Eric-Hou1997/Tech-Card-Manager/releases/latest"
	cardReleasePage = "https://github.com/Eric-Hou1997/Tech-Card-Manager/releases/latest"
)

var cardVersionPattern = regexp.MustCompile(`^v?(\d+)\.(\d+)\.(\d+)$`)

type cardGitHubRelease struct {
	TagName     string `json:"tag_name"`
	HTMLURL     string `json:"html_url"`
	Draft       bool   `json:"draft"`
	Prerelease  bool   `json:"prerelease"`
	PublishedAt string `json:"published_at"`
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

func handleCardUpdate(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		writeJSONStatus(w, http.StatusMethodNotAllowed, map[string]string{"error": "仅支持 GET"})
		return
	}
	client := &http.Client{Timeout: 12 * time.Second}
	req, err := http.NewRequest(http.MethodGet, cardReleaseAPI, nil)
	if err != nil {
		writeJSONStatus(w, 500, map[string]string{"error": err.Error()})
		return
	}
	req.Header.Set("Accept", "application/vnd.github+json")
	req.Header.Set("User-Agent", "Tech-Card-Manager/"+appVersion)
	resp, err := client.Do(req)
	if err != nil {
		writeJSONStatus(w, http.StatusBadGateway, map[string]string{"error": "无法连接 GitHub：" + err.Error()})
		return
	}
	defer resp.Body.Close()
	if resp.StatusCode == http.StatusNotFound {
		writeJSONStatus(w, http.StatusNotFound, map[string]string{"error": "GitHub 尚未发布正式版本"})
		return
	}
	if resp.StatusCode != http.StatusOK {
		writeJSONStatus(w, http.StatusBadGateway, map[string]string{"error": fmt.Sprintf("GitHub 更新检查失败（HTTP %d）", resp.StatusCode)})
		return
	}
	var release cardGitHubRelease
	if err := json.NewDecoder(io.LimitReader(resp.Body, 2<<20)).Decode(&release); err != nil {
		writeJSONStatus(w, http.StatusBadGateway, map[string]string{"error": "GitHub 更新信息无效：" + err.Error()})
		return
	}
	if release.Draft || release.Prerelease || cardVersionPattern.FindStringSubmatch(release.TagName) == nil {
		writeJSONStatus(w, http.StatusBadGateway, map[string]string{"error": "GitHub 最新发布不是可用的正式 vX.Y.Z 版本"})
		return
	}
	cmp, err := compareCardVersion(appVersion, release.TagName)
	if err != nil {
		writeJSONStatus(w, http.StatusBadGateway, map[string]string{"error": err.Error()})
		return
	}
	writeJSON(w, map[string]interface{}{
		"current_version": "v" + appVersion, "latest_version": release.TagName, "available": cmp < 0,
		"release_url": cardReleasePage, "published_at": release.PublishedAt,
		"portable_directory": portableRootDir(),
	})
}
