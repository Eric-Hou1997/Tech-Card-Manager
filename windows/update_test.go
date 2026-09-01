package main

import "testing"

func TestCompareCardVersion(t *testing.T) {
	cases := []struct {
		left, right string
		want        int
	}{
		{"4.0.0", "v4.0.0", 0},
		{"4.0.0", "v4.2.0", -1},
		{"5.0.0", "v4.9.9", 1},
	}
	for _, tc := range cases {
		got, err := compareCardVersion(tc.left, tc.right)
		if err != nil || got != tc.want {
			t.Fatalf("compareCardVersion(%q, %q) = %d, %v; want %d", tc.left, tc.right, got, err, tc.want)
		}
	}
	if _, err := compareCardVersion("latest", "v4.0.0"); err == nil {
		t.Fatal("non-semver tag must be rejected")
	}
}
