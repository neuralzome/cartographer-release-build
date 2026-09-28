//! What gets built, at which commit, and the VERSION file that records it.
//!
//! Cartographer and Ceres are the mapping server's pins
//! (mapping_server/devops/docker/Dockerfile), so the Cartographer that writes a
//! map's .pbstream is the one that reads it. abseil and protobuf are the
//! versions Debian bookworm ships and the server links, built here instead of
//! taken from apt: install.sh puts protobuf 27.2 in /usr/local for ubr's C++
//! nodes, and Cartographer must never see it.

use std::collections::BTreeMap;

use crate::platform::Platform;

pub struct Pin {
    /// The build's name: its source and build directories, its log, and its
    /// key in VERSION.
    pub name: &'static str,
    pub url: &'static str,
    /// A tag or a commit. Either is fetched as one commit.
    pub git_ref: &'static str,
}

pub const ABSEIL: Pin = Pin {
    name: "abseil",
    url: "https://github.com/abseil/abseil-cpp.git",
    git_ref: "20220623.1",
};

pub const PROTOBUF: Pin = Pin {
    name: "protobuf",
    url: "https://github.com/protocolbuffers/protobuf.git",
    git_ref: "v21.12",
};

/// Cartographer still uses the LocalParameterization API that 2.2 removed.
pub const CERES: Pin = Pin {
    name: "ceres",
    url: "https://github.com/ceres-solver/ceres-solver.git",
    git_ref: "2.1.0",
};

pub const CARTOGRAPHER: Pin = Pin {
    name: "cartographer",
    url: "https://github.com/cartographer-project/cartographer.git",
    git_ref: "877157a0d91788a7700221d87232d412cb3c1ef4",
};

/// In build order; the first three are the dependencies tarball.
pub const DEPENDENCIES: [&Pin; 3] = [&ABSEIL, &PROTOBUF, &CERES];

/// What `protoc --version` prints for PROTOBUF: from 21.x on the tag drops the
/// language's major version, and protoc still reports it.
pub fn protoc_version() -> String {
    format!("libprotoc 3.{}", PROTOBUF.git_ref.trim_start_matches('v'))
}

/// Written to <prefix>/VERSION and packed in the cartographer tarball, so an
/// installation says what it is. install.sh compares it with its own pins.
pub fn version_file(platform: &Platform, carto_cxx_flags_release: &str) -> String {
    let mut text = format!("platform={}\n", platform.tag());
    for pin in [&CARTOGRAPHER, &CERES, &PROTOBUF, &ABSEIL] {
        text.push_str(&format!("{}={}\n", pin.name, pin.git_ref));
    }
    text.push_str(&format!("cartographer_cxx_flags_release={carto_cxx_flags_release}\n"));
    text
}

pub fn parse_version_file(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protoc_reports_the_language_major_version() {
        assert_eq!(protoc_version(), "libprotoc 3.21.12");
    }

    #[test]
    fn version_file_reads_back() {
        let platform = Platform { id: "ubuntu".into(), version_id: "22.04".into(), arch: "aarch64" };
        let parsed = parse_version_file(&version_file(&platform, "-O3 -DNDEBUG"));
        assert_eq!(parsed["platform"], "ubuntu22.04-aarch64");
        assert_eq!(parsed["cartographer"], CARTOGRAPHER.git_ref);
        assert_eq!(parsed["abseil"], "20220623.1");
        assert_eq!(parsed["cartographer_cxx_flags_release"], "-O3 -DNDEBUG");
    }
}
