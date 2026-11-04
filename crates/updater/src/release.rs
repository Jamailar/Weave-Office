//! Versions, platforms and the GitHub release that announces an update.
use serde::Deserialize;
use std::fmt;

/// `major.minor.patch`, as in the release tags (`v0.1.3`). Pre-release tags are not updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text
            .strip_prefix('v')
            .or_else(|| text.strip_prefix('V'))
            .unwrap_or(text);
        let mut parts = text.split('.');
        let mut number = || {
            let part = parts.next()?;
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            part.parse().ok()
        };
        let version = Self {
            major: number()?,
            minor: number()?,
            patch: number()?,
        };
        parts.next().is_none().then_some(version)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Windows,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arch {
    Arm64,
    X64,
    X86,
}

/// The installer a running copy updates itself with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Platform {
    pub os: Os,
    pub arch: Arch,
}

impl Platform {
    /// The platform this binary was built for, when releases publish an installer for it.
    pub fn current() -> Option<Self> {
        let os = if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(windows) {
            Os::Windows
        } else {
            return None;
        };
        let arch = if cfg!(target_arch = "aarch64") {
            Arch::Arm64
        } else if cfg!(target_arch = "x86_64") {
            Arch::X64
        } else if cfg!(target_arch = "x86") {
            Arch::X86
        } else {
            return None;
        };
        if os == Os::MacOs && arch == Arch::X86 {
            return None;
        }
        Some(Self { os, arch })
    }

    /// The release asset name, matching `scripts/build-windows-app.py` and the DMG names.
    pub fn package_name(self, version: &Version) -> String {
        let arch = match self.arch {
            Arch::Arm64 => "arm64",
            Arch::X64 => "x64",
            Arch::X86 => "x86",
        };
        match self.os {
            Os::MacOs => format!("WeaveOffice-{version}-macos-{arch}.dmg"),
            Os::Windows => format!("WeaveOffice-{version}-windows-{arch}-setup.exe"),
        }
    }
}

pub const CHECKSUMS: &str = "SHA256SUMS.txt";
pub const SIGNATURE: &str = "SHA256SUMS.txt.sig";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

/// The latest published release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub notes: String,
    /// The release's web page, for reading the notes or downloading by hand.
    pub page: String,
    pub assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

impl Release {
    /// Read `GET /repos/{owner}/{repo}/releases/latest`.
    pub fn from_github(json: &[u8]) -> Result<Self, String> {
        let release: GitHubRelease =
            serde_json::from_slice(json).map_err(|error| format!("release data: {error}"))?;
        if release.draft || release.prerelease {
            return Err(format!("{} is not a published release", release.tag_name));
        }
        let version = Version::parse(&release.tag_name)
            .ok_or_else(|| format!("release tag {} is not a version", release.tag_name))?;
        Ok(Self {
            version,
            notes: release.body.unwrap_or_default().trim().to_owned(),
            page: https(&release.html_url).unwrap_or_default(),
            assets: release
                .assets
                .into_iter()
                .filter_map(|asset| {
                    Some(Asset {
                        url: https(&asset.browser_download_url)?,
                        name: asset.name,
                        size: asset.size,
                    })
                })
                .collect(),
        })
    }

    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|asset| asset.name == name)
    }
}

fn https(url: &str) -> Option<String> {
    url.starts_with("https://").then(|| url.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_and_order() {
        let v = |s| Version::parse(s).unwrap();
        assert_eq!(v("v0.1.2").to_string(), "0.1.2");
        assert_eq!(
            v("1.10.0"),
            Version {
                major: 1,
                minor: 10,
                patch: 0
            }
        );
        assert!(v("0.1.10") > v("0.1.9"));
        assert!(v("0.2.0") > v("0.1.99"));
        assert!(v("1.0.0") > v("0.9.9"));
        for bad in [
            "",
            "v",
            "0.1",
            "0.1.2.3",
            "0.1.2-beta",
            "0.1.+2",
            "a.b.c",
            "0..1",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn package_names_match_published_installers() {
        let version = Version::parse("0.1.2").unwrap();
        let name = |os, arch| Platform { os, arch }.package_name(&version);
        assert_eq!(
            name(Os::MacOs, Arch::Arm64),
            "WeaveOffice-0.1.2-macos-arm64.dmg"
        );
        assert_eq!(
            name(Os::MacOs, Arch::X64),
            "WeaveOffice-0.1.2-macos-x64.dmg"
        );
        assert_eq!(
            name(Os::Windows, Arch::X86),
            "WeaveOffice-0.1.2-windows-x86-setup.exe"
        );
        assert_eq!(
            name(Os::Windows, Arch::Arm64),
            "WeaveOffice-0.1.2-windows-arm64-setup.exe"
        );
    }

    #[test]
    fn github_release_is_read_and_drafts_are_refused() {
        let json = br#"{
            "tag_name": "v0.1.3", "body": " Fixes \n", "draft": false, "prerelease": false,
            "html_url": "https://github.com/example/weave-office/releases/tag/v0.1.3",
            "assets": [
                {"name": "SHA256SUMS.txt", "size": 10,
                 "browser_download_url": "https://github.com/example/weave-office/releases/download/v0.1.3/SHA256SUMS.txt"},
                {"name": "plain.txt", "size": 1, "browser_download_url": "http://example.com/plain.txt"}
            ]
        }"#;
        let release = Release::from_github(json).unwrap();
        assert_eq!(release.version.to_string(), "0.1.3");
        assert_eq!(release.notes, "Fixes");
        assert_eq!(release.asset(CHECKSUMS).unwrap().size, 10);
        assert!(release.asset("plain.txt").is_none(), "only https downloads");
        let draft = br#"{"tag_name": "v0.1.3", "html_url": "https://x", "draft": true}"#;
        assert!(Release::from_github(draft).is_err());
        let pre = br#"{"tag_name": "v0.1.3", "html_url": "https://x", "prerelease": true}"#;
        assert!(Release::from_github(pre).is_err());
    }
}
