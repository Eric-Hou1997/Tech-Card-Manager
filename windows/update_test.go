package main

import "testing"

func TestCompareCardVersion(t *testing.T) {
	cases := []struct {
		left, right string
		want        int
	}{
		{"4.0.3", "v4.0.3", 0},
		{"4.0.3", "v4.2.0", -1},
		{"4.0.3", "v4.0.0", 1},
		{"5.0.0", "v4.9.9", 1},
	}
	for _, tc := range cases {
		got, err := compareCardVersion(tc.left, tc.right)
		if err != nil || got != tc.want {
			t.Fatalf("compareCardVersion(%q, %q) = %d, %v; want %d", tc.left, tc.right, got, err, tc.want)
		}
	}
	if _, err := compareCardVersion("latest", "v4.0.3"); err == nil {
		t.Fatal("non-semver tag must be rejected")
	}
}

func TestSelectCardUpdateAssetRequiresCanonicalName(t *testing.T) {
	release := cardGitHubRelease{
		TagName: "v4.0.3",
		Assets: []cardGitHubReleaseAsset{
			{Name: "TCM-v4.0.3-MacOS-AArch64-APP.zip", BrowserDownloadURL: "wrong-platform"},
			{Name: "TCM-v4.0.3-Windows-x64-EXE.zip", BrowserDownloadURL: "package"},
		},
	}
	asset, err := selectCardUpdateAsset(release)
	if err != nil {
		t.Fatal(err)
	}
	if asset.BrowserDownloadURL != "package" {
		t.Fatalf("selected wrong asset: %#v", asset)
	}

	release.Assets = []cardGitHubReleaseAsset{{Name: "Tech-Card-Manager-Windows-x64-v4.0.3.zip"}}
	if _, err := selectCardUpdateAsset(release); err == nil {
		t.Fatal("legacy or ambiguous archive names must not satisfy the update selector")
	}
}
