//! The frameworks under test: each one is an app in `apps/<name>` with its own build. An app
//! shows one scenario per run, which the command line names. An edition of a framework is another
//! build of the same sources: `dioxus-skia` builds `apps/dioxus` with the `skia` feature into
//! `target/skia`; `gpui-kit-fast` and `freya-056a5b4` need dependency versions that cannot share
//! a lock file with their framework, so each is its own app directory whose `src` links to the
//! framework's sources.

use std::path::{Path, PathBuf};

/// One framework and the app that represents it.
#[derive(Debug, Clone)]
pub struct Framework {
    /// The short name in results and plots.
    pub name: &'static str,
    /// The app directory under `apps/`; its binary is `<app>-bench`.
    pub app: &'static str,
    /// The target directory under the app's `target/` for a build with other features, such as
    /// `skia`, or empty for the default build.
    pub build: &'static str,
    /// The app reports frames, handled input and phase spans through fakeland app marks.
    pub marks: bool,
    /// The variants of the stateful scenarios (table and ticker) that the app implements.
    pub variants: &'static [&'static str],
}

const SIGNALS: &[&str] = &["fine", "coarse", "shared"];
const RETAINED: &[&str] = &["fine", "coarse"];
const IMMEDIATE: &[&str] = &["default"];

/// Every known framework.
pub const ALL: &[Framework] = &[
    Framework {
        name: "zgui",
        app: "zgui",
        build: "",
        marks: true,
        variants: SIGNALS,
    },
    Framework {
        name: "gpui-kit",
        app: "gpui-kit",
        build: "",
        marks: false,
        variants: RETAINED,
    },
    Framework {
        name: "gpui-kit-fast",
        app: "gpui-kit-fast",
        build: "",
        marks: false,
        variants: RETAINED,
    },
    Framework {
        name: "freya",
        app: "freya",
        build: "",
        marks: false,
        variants: SIGNALS,
    },
    Framework {
        name: "freya-056a5b4",
        app: "freya-056a5b4",
        build: "",
        marks: false,
        variants: SIGNALS,
    },
    Framework {
        name: "vizia",
        app: "vizia",
        build: "",
        marks: false,
        variants: SIGNALS,
    },
    Framework {
        name: "floem",
        app: "floem",
        build: "",
        marks: false,
        variants: SIGNALS,
    },
    Framework {
        name: "iced",
        app: "iced",
        build: "",
        marks: false,
        variants: RETAINED,
    },
    Framework {
        name: "egui",
        app: "egui",
        build: "",
        marks: false,
        variants: IMMEDIATE,
    },
    Framework {
        name: "dioxus",
        app: "dioxus",
        build: "",
        marks: false,
        variants: SIGNALS,
    },
    Framework {
        name: "dioxus-skia",
        app: "dioxus",
        build: "skia",
        marks: false,
        variants: SIGNALS,
    },
    Framework {
        name: "bough",
        app: "bough",
        build: "",
        marks: false,
        variants: RETAINED,
    },
];

impl Framework {
    /// Returns the framework with a name.
    pub fn by_name(name: &str) -> Option<&'static Framework> {
        ALL.iter().find(|f| f.name == name)
    }

    /// Returns the binary name of the app, `<app>-bench`.
    pub fn binary(&self) -> String {
        format!("{}-bench", self.app)
    }

    /// Returns the release binary of the app's build.
    pub fn binary_path(&self, root: &Path) -> PathBuf {
        root.join("apps")
            .join(self.app)
            .join("target")
            .join(self.build)
            .join("release")
            .join(self.binary())
    }
}

/// Returns the repository root.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}
